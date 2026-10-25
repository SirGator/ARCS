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

use super::{CompletedTask, RuntimeConfig, config::invalid};
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
pub(super) const PENDING_COUNT: &str = "SELECT COUNT(*) FROM runtime_jobs WHERE state IN ('pending','running','failed','awaiting_approval')";
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
        Ok(Self {
            connection,
            operator,
            ids,
            _lock: lock,
        })
    }
    pub fn pending(&self) -> Result<usize, HostError> {
        self.connection
            .query_row(PENDING_COUNT, [], |row| row.get::<_, i64>(0))
            .map(|count| count as usize)
            .map_err(db)
    }
    pub fn next(&self, cursor: u64) -> Result<Option<(u64, String, VersionId)>, HostError> {
        self.connection.query_row(&format!("SELECT sequence,route_id,input_version FROM runtime_jobs WHERE {ACTIVE} ORDER BY (sequence<=?1),sequence LIMIT 1"),[cursor.min(i64::MAX as u64) as i64],|row| Ok((row.get::<_,i64>(0)? as u64,row.get(1)?,VersionId(row.get(2)?)))).optional().map_err(db)
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
        id: capability.clone(),
        contract: CapabilityContract::Input {
            emits: vec![SchemaId("arcs.input.host_runtime.v1".into())],
        },
        required_permissions: vec![],
    }];
    for route in &config.routes {
        if let Some(goal) = &route.goal {
            for source in &goal.sources {
                if let Some(query) = &source.query {
                    capabilities.push(CapabilityDescriptor {
                        id: CapabilityId(format!("runtime.query.{}.{}", route.id, source.id)),
                        contract: CapabilityContract::Input {
                            emits: vec![query.schema_id.clone()],
                        },
                        required_permissions: vec![],
                    });
                }
            }
        }
    }
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
