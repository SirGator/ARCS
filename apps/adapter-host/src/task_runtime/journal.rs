mod control;
mod deadlines;
mod dependencies;
pub(super) mod dispatch;
mod planning;
mod queries;
mod reasoning;
mod scheduling;
pub(super) use planning::capacity_query;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use arcs::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, ProducerClass,
};
use arcs::core::{SchemaId, SourceKind, SystemClock, TrustLevel, VersionId};
use arcs::input::{InputMessage, InputService};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use super::{CompletedTask, RuntimeConfig, config::invalid, goal::GoalTask};
use crate::config::RUNTIME_ID;
use crate::{Host, HostError};

pub(super) fn db(error: rusqlite::Error) -> HostError {
    HostError::Store(error.into())
}
fn io(path: &Path, source: std::io::Error) -> HostError {
    HostError::Io {
        path: path.into(),
        source,
    }
}

pub(super) struct Journal {
    pub connection: Connection,
    pub operator: AdapterRegistry,
    pub ids: arcs::core::SequenceIdGenerator,
    _lock: File,
}
pub(super) const ACTIVE: &str = "state IN ('pending','running','failed','awaiting_approval')";
pub(super) fn active(include_failed: bool) -> &'static str {
    if include_failed {
        ACTIVE
    } else {
        "state IN ('pending','running','awaiting_approval')"
    }
}
pub(super) fn pending_query(include_failed: bool) -> String {
    format!(
        "SELECT COUNT(*) FROM runtime_jobs WHERE {}",
        active(include_failed)
    )
}
impl Journal {
    pub fn open(host: &Host, config: &RuntimeConfig, identity: Value) -> Result<Self, HostError> {
        let absolute = std::env::current_dir()
            .map_err(|e| io(Path::new("."), e))?
            .join(&config.journal);
        let parent = absolute
            .parent()
            .ok_or_else(|| invalid("journal needs a parent"))?;
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        let parent = parent.canonicalize().map_err(|e| io(parent, e))?;
        let path = parent.join(
            absolute
                .file_name()
                .ok_or_else(|| invalid("journal needs a filename"))?,
        );
        // Resolve existing symlinks too, so aliases share the same lock and identity.
        let path = if path.exists() {
            path.canonicalize().map_err(|e| io(&path, e))?
        } else {
            path
        };
        let lock_path = PathBuf::from(format!("{}.lock", path.display()));
        let lock = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| io(&lock_path, e))?;
        lock.try_lock().map_err(|error| {
            invalid(format!(
                "runtime journal already in use or cannot be locked: {error}"
            ))
        })?;
        let mut connection = Connection::open(&path).map_err(db)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(db)?;
        connection.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS runtime_installation(singleton INTEGER PRIMARY KEY CHECK(singleton=1),identity TEXT NOT NULL,cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0));
            CREATE TABLE IF NOT EXISTS runtime_jobs(sequence INTEGER PRIMARY KEY AUTOINCREMENT,route_id TEXT NOT NULL,input_version TEXT NOT NULL,source_sequence INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN ('pending','running','failed','ready_for_review')),error TEXT,result_json TEXT,UNIQUE(route_id,input_version));
            CREATE INDEX IF NOT EXISTS runtime_pending ON runtime_jobs(state,sequence);").map_err(db)?;
        let identity = serde_json::to_string(
            &json!({"journal":path,"core_database":host.database_path(),"definition":identity}),
        )
        .map_err(|e| invalid(e.to_string()))?;
        let prior: Option<String> = connection
            .query_row(
                "SELECT identity FROM runtime_installation WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(db)?;
        if prior.as_ref().is_some_and(|previous| previous != &identity) {
            return Err(invalid(
                "journal definition changed; use a new runtime ID and journal",
            ));
        }
        let (anchor, operator, ids) = bind_definition(
            host,
            config,
            serde_json::from_str(&identity).map_err(|e| invalid(e.to_string()))?,
        )?;
        // Bind this operational queue to the persistent Core definition as well.
        let binding = json!({"identity":identity,"anchor":anchor});
        // The anchor is stable for this Core and is stored separately from the configuration fingerprint.
        connection.execute_batch("CREATE TABLE IF NOT EXISTS runtime_core_binding(singleton INTEGER PRIMARY KEY CHECK(singleton=1),binding TEXT NOT NULL);").map_err(db)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO runtime_installation(singleton,identity) VALUES(1,?1)",
                [&identity],
            )
            .map_err(db)?;
        let binding = binding.to_string();
        transaction
            .execute(
                "INSERT OR IGNORE INTO runtime_core_binding VALUES(1,?1)",
                [&binding],
            )
            .map_err(db)?;
        let previous: String = transaction
            .query_row("SELECT binding FROM runtime_core_binding", [], |row| {
                row.get(0)
            })
            .map_err(db)?;
        if previous != binding {
            return Err(invalid("runtime journal belongs to another Core store"));
        }
        transaction.commit().map_err(db)?;
        // Upgrade the operational queue without changing its identity or sequence IDs.
        let sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='runtime_jobs'",
                [],
                |row| row.get(0),
            )
            .map_err(db)?;
        if !sql.contains("goal_json") {
            connection.execute_batch("BEGIN IMMEDIATE;
                DROP INDEX IF EXISTS runtime_pending;
                ALTER TABLE runtime_jobs RENAME TO runtime_jobs_previous;
                CREATE TABLE runtime_jobs(sequence INTEGER PRIMARY KEY AUTOINCREMENT,route_id TEXT NOT NULL,input_version TEXT NOT NULL,source_sequence INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN ('pending','running','failed','ready_for_review','awaiting_approval','completed','blocked','rejected','budget_exhausted','cancelled')),error TEXT,result_json TEXT,goal_json TEXT,UNIQUE(route_id,input_version));
                INSERT INTO runtime_jobs(sequence,route_id,input_version,source_sequence,state,error,result_json) SELECT sequence,route_id,input_version,source_sequence,state,error,result_json FROM runtime_jobs_previous;
                DROP TABLE runtime_jobs_previous;
                CREATE INDEX runtime_pending ON runtime_jobs(state,sequence);
                COMMIT;").map_err(db)?;
        }
        scheduling::initialize(&connection)?;
        dependencies::initialize(&connection)?;
        planning::initialize(&connection)?;
        reasoning::initialize(&connection)?;
        control::initialize(&connection)?;
        dispatch::initialize(&connection, config)?;
        deadlines::initialize(&connection)?;
        Ok(Self {
            connection,
            operator,
            ids,
            _lock: lock,
        })
    }
    pub fn pending(&self, include_failed: bool) -> Result<usize, HostError> {
        self.connection
            .query_row(&pending_query(include_failed), [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count as usize)
            .map_err(db)
    }
    pub fn start(&self, sequence: u64) -> Result<(), HostError> {
        self.connection
            .execute(
                "UPDATE runtime_jobs SET state='running',error=NULL WHERE sequence=?1",
                [sequence.min(i64::MAX as u64) as i64],
            )
            .map_err(db)?;
        Ok(())
    }
    pub fn fail(&self, sequence: u64, error: &HostError) -> Result<(), HostError> {
        self.connection
            .execute(
                "UPDATE runtime_jobs SET state='failed',error=?2 WHERE sequence=?1",
                params![
                    sequence.min(i64::MAX as u64) as i64,
                    error.to_string().chars().take(2000).collect::<String>()
                ],
            )
            .map_err(db)?;
        Ok(())
    }
    pub fn complete(&self, result: &CompletedTask) -> Result<(), HostError> {
        self.connection.execute("UPDATE runtime_jobs SET state='ready_for_review',error=NULL,result_json=?2 WHERE sequence=?1",params![result.sequence.min(i64::MAX as u64) as i64,serde_json::to_string(result).map_err(|e| invalid(e.to_string()))?]).map_err(db)?;
        Ok(())
    }
}
fn bind_definition(
    host: &Host,
    config: &RuntimeConfig,
    identity: Value,
) -> Result<(VersionId, AdapterRegistry, arcs::core::SequenceIdGenerator), HostError> {
    let mut registry = AdapterRegistry::new();
    let adapter = AdapterId(RUNTIME_ID.into());
    let capability = CapabilityId("runtime.definition".into());
    let mut capabilities = vec![CapabilityDescriptor {
        description: None,
        id: capability.clone(),
        contract: CapabilityContract::Input {
            emits: vec![SchemaId("arcs.input.host_runtime.v1".into())],
        },
        required_permissions: vec![],
    }];
    capabilities.extend(queries::query_capabilities(config));
    let enabled_capabilities = capabilities.iter().map(|cap| cap.id.clone()).collect();
    registry
        .register(
            AdapterManifest {
                protocol_version: 1,
                adapter_id: adapter.clone(),
                adapter_version: "1".into(),
                capabilities,
            },
            AdapterGrant {
                adapter_id: adapter.clone(),
                producer_class: ProducerClass::System,
                enabled_capabilities,
                granted_permissions: vec![],
                assigned_trust: TrustLevel::High,
                ingress_source_kind: Some(SourceKind::Internal),
                max_payload_bytes: 65_536,
                max_external_reference_bytes: 256,
                data_limits: None,
                reasoning_limits: None,
                input_data_limits: None,
                reasoning_limits_v2: None,
            },
            host.schemas(),
            host.store(),
        )
        .map_err(HostError::Registration)?;
    let mut ids = host
        .store()
        .durable_id_generator("task-runtime")
        .map_err(HostError::Store)?;
    let artifact = InputService::new(
        &registry,
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .ingest(
        &adapter,
        InputMessage {
            capability_id: capability,
            external_subject: Some(config.runtime_id.clone()),
            external_reference: format!("runtime:{}:definition", config.runtime_id),
            payload: json!({"runtime_id":config.runtime_id,"definition":identity}),
        },
    )
    .map_err(|source| HostError::Input {
        request: config.runtime_id.clone(),
        source,
    })?;
    Ok((artifact.version_id, registry, ids))
}

// Goal checkpoint serialization belongs to the journal, not the goal engine.
impl Journal {
    pub(in crate::task_runtime) fn goals_for_report(&self) -> Result<Vec<GoalTask>, HostError> {
        let mut query = self.connection.prepare("SELECT goal_json FROM runtime_jobs WHERE goal_json IS NOT NULL ORDER BY (state IN ('pending','running','failed','awaiting_approval')) DESC,sequence DESC LIMIT 50").map_err(db)?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db)?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(db)?)
                .map_err(|e| invalid(format!("invalid goal checkpoint: {e}")))
        })
        .collect()
    }
    pub fn goals_after(&self, sequence: u64, limit: usize) -> Result<Vec<GoalTask>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("goal page size must be 1..1000"));
        }
        let mut query = self.connection.prepare("SELECT goal_json FROM runtime_jobs WHERE sequence>?1 AND goal_json IS NOT NULL ORDER BY sequence LIMIT ?2").map_err(db)?;
        let rows = query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                |row| row.get::<_, String>(0),
            )
            .map_err(db)?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(db)?)
                .map_err(|e| invalid(format!("invalid goal checkpoint: {e}")))
        })
        .collect()
    }

    pub(in crate::task_runtime) fn saved_goal(
        &self,
        sequence: u64,
    ) -> Result<Option<GoalTask>, HostError> {
        let value: Option<Option<String>> = self
            .connection
            .query_row(
                "SELECT goal_json FROM runtime_jobs WHERE sequence=?1",
                [sequence.min(i64::MAX as u64) as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(db)?;
        value
            .flatten()
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| invalid(format!("invalid goal checkpoint: {e}")))
            })
            .transpose()
    }

    pub(in crate::task_runtime) fn save_goal(&self, task: &GoalTask) -> Result<(), HostError> {
        let json = serde_json::to_string(task).map_err(|e| invalid(e.to_string()))?;
        self.connection
            .execute(
                "UPDATE runtime_jobs SET state=?2,goal_json=?3,error=NULL WHERE sequence=?1",
                params![task.sequence as i64, task.state.queue_state(), json],
            )
            .map_err(db)?;
        Ok(())
    }
}

impl Journal {
    pub(super) fn source_sequence(&self, sequence: u64) -> Result<u64, HostError> {
        self.connection
            .query_row(
                "SELECT source_sequence FROM runtime_jobs WHERE sequence=?1",
                [sequence as i64],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )
            .map_err(db)
    }
}
