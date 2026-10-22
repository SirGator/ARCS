//! Allgemeine Dateiquelle mit unveränderlichen SQLite-Snapshots.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId,
    DATA_REFERENCE_SCHEMA, DataChunk, DataRead, DataReference, MAX_DATA_OBJECT_BYTES,
    ObservationMessage, SchemaId,
};
use arcs_adapter_sdk::{Adapter, AdapterError, PendingObservation};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

pub const CAPABILITY: &str = "data.observe";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub adapter_id: AdapterId,
    pub resources: Vec<Resource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub id: String,
    pub media_type: String,
    pub path: PathBuf,
}

fn manifest(id: AdapterId) -> AdapterManifest {
    AdapterManifest {
        protocol_version: 1,
        adapter_id: id,
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId(CAPABILITY.into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId(DATA_REFERENCE_SCHEMA.into())],
            },
            required_permissions: vec!["file.read".into()],
        }],
    }
}

fn storage(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(error.to_string())
}

pub struct FileData {
    manifest: AdapterManifest,
    database: Connection,
}

impl FileData {
    /// Pfade im Profil werden relativ zur Profildatei aufgelöst. Nur diese
    /// Betreiberdateien werden importiert; der Host übermittelt keine Pfade.
    pub fn open(profile_path: &Path, database_path: &Path) -> Result<Self, String> {
        let profile_path = profile_path
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let profile: Profile = serde_json::from_slice(
            &std::fs::read(&profile_path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let manifest = manifest(profile.adapter_id);
        manifest.validate().map_err(|error| format!("{error:?}"))?;
        if profile.resources.is_empty() || profile.resources.len() > 256 {
            return Err("profile requires 1..256 resources".into());
        }
        let mut ids = HashSet::new();
        for resource in &profile.resources {
            let reference = DataReference {
                id: resource.id.clone(),
                media_type: resource.media_type.clone(),
                size_bytes: 0,
            };
            if !reference.is_valid()
                || resource.path.as_os_str().is_empty()
                || !ids.insert(&resource.id)
            {
                return Err("invalid or duplicate resource definition".into());
            }
        }
        let mut database = Connection::open(database_path).map_err(|error| error.to_string())?;
        database.execute_batch("
            CREATE TABLE IF NOT EXISTS data_source (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), adapter_id TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS resources (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE, media_type TEXT NOT NULL,
                size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0 AND size_bytes <= 67108864),
                data BLOB NOT NULL CHECK (length(data) = size_bytes),
                acknowledged INTEGER NOT NULL DEFAULT 0
            );
        ").map_err(|error| error.to_string())?;
        let transaction = database.transaction().map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO data_source VALUES (1, ?1)",
                [&manifest.adapter_id.0],
            )
            .map_err(|error| error.to_string())?;
        let owner: String = transaction
            .query_row(
                "SELECT adapter_id FROM data_source WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if owner != manifest.adapter_id.0 {
            return Err("database belongs to another adapter".into());
        }
        for resource in profile.resources {
            let path = profile_path.parent().unwrap().join(&resource.path);
            let file = std::fs::File::open(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let metadata = file.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_file() || metadata.len() > MAX_DATA_OBJECT_BYTES {
                return Err("resource must be a regular file of at most 64 MiB".into());
            }
            let mut bytes = Vec::new();
            file.take(MAX_DATA_OBJECT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() as u64 > MAX_DATA_OBJECT_BYTES {
                return Err("resource exceeds 64 MiB".into());
            }
            let previous: Option<bool> = transaction.query_row(
                "SELECT media_type = ?2 AND size_bytes = ?3 AND data = ?4 FROM resources WHERE id = ?1",
                params![resource.id, resource.media_type, bytes.len() as i64, bytes], |row| row.get(0)
            ).optional().map_err(|error| error.to_string())?;
            match previous {
                Some(true) => {}
                Some(false) => {
                    return Err(format!(
                        "resource identity conflict: {} needs a new ID",
                        resource.id
                    ));
                }
                None => {
                    transaction.execute("INSERT INTO resources (id, media_type, size_bytes, data) VALUES (?1, ?2, ?3, ?4)",
                    params![resource.id, resource.media_type, bytes.len() as i64, bytes]).map_err(|error| error.to_string())?;
                }
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(Self { manifest, database })
    }
}

impl Adapter for FileData {
    fn manifest(&self) -> AdapterManifest {
        self.manifest.clone()
    }

    fn poll(&mut self) -> Result<Option<PendingObservation>, AdapterError> {
        let reference: Option<DataReference> = self.database.query_row(
            "SELECT id, media_type, size_bytes FROM resources WHERE acknowledged = 0 ORDER BY sequence LIMIT 1", [],
            |row| Ok(DataReference { id: row.get(0)?, media_type: row.get(1)?, size_bytes: row.get::<_, u32>(2)? as u64 })
        ).optional().map_err(storage)?;
        reference
            .map(|reference| {
                Ok(PendingObservation {
                    message_id: format!("data:{}", reference.id),
                    observation: ObservationMessage {
                        capability_id: CapabilityId(CAPABILITY.into()),
                        external_subject: Some(reference.id.clone()),
                        external_reference: format!("data:{}", reference.id),
                        payload: serde_json::to_value(reference).map_err(storage)?,
                    },
                })
            })
            .transpose()
    }

    fn acknowledge(&mut self, message_id: &str) -> Result<(), AdapterError> {
        let id = message_id
            .strip_prefix("data:")
            .ok_or_else(|| AdapterError::rejected("invalid acknowledgement"))?;
        let changed = self
            .database
            .execute("UPDATE resources SET acknowledged = 1 WHERE id = ?1", [id])
            .map_err(storage)?;
        if changed == 0 {
            return Err(AdapterError::rejected("unknown acknowledgement"));
        }
        Ok(())
    }

    fn read_data(&mut self, request: DataRead) -> Result<DataChunk, AdapterError> {
        if !request.is_valid()
            || request.capability.adapter_id != self.manifest.adapter_id
            || request.capability.capability_id.0 != CAPABILITY
        {
            return Err(AdapterError::rejected("data contract mismatch"));
        }
        let bytes: Option<Vec<u8>> = self.database.query_row(
            "SELECT CASE WHEN size_bytes = 0 THEN X'' ELSE substr(data, ?4, ?5) END FROM resources WHERE id = ?1 AND media_type = ?2 AND size_bytes = ?3",
            params![request.reference.id, request.reference.media_type, request.reference.size_bytes as i64, (request.offset + 1) as i64, request.expected_bytes().unwrap() as i64],
            |row| row.get(0)
        ).optional().map_err(storage)?;
        let bytes =
            bytes.ok_or_else(|| AdapterError::rejected("unknown or changed data reference"))?;
        Ok(DataChunk {
            reference_id: request.reference.id,
            offset: request.offset,
            bytes,
        })
    }
}
