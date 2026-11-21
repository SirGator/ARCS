//! Bounded, operator-scoped source retrieval through the Request manager.
use std::path::Path;

use arcs_adapter_protocol::{
    AdapterId, CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef,
    RequestInvocation, RequestResponse, SchemaId,
};
use arcs_adapter_sdk::AdapterError;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::git_edit::RepositoryScope;

const CAPABILITY: &str = "source.read";
const REQUEST: &str = "arcs.workspace_source_request.v1";
const RESPONSE: &str = "arcs.workspace_source.v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    revision: String,
    path: String,
}

pub(crate) fn capability() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId(CAPABILITY.into()),
        description: None,
        contract: CapabilityContract::Request {
            accepts: vec![SchemaId(REQUEST.into())],
            emits: vec![SchemaId(RESPONSE.into())],
        },
        required_permissions: vec!["workspace.source_read".into()],
    }
}

pub(crate) fn initialize(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS source_reads (invocation_id TEXT PRIMARY KEY, input TEXT NOT NULL, payload TEXT NOT NULL);",
    ).map_err(|error| error.to_string())
}

pub(crate) fn request(
    db: &Connection,
    adapter: &AdapterId,
    workspace: &Path,
    scope: &RepositoryScope,
    invocation: RequestInvocation,
) -> Result<RequestResponse, AdapterError> {
    if invocation.capability != CapabilityRef::new(&adapter.0, CAPABILITY)
        || invocation.request_schema_id.0 != REQUEST
        || invocation.response_schema_id.0 != RESPONSE
        || invocation.request_version_id.0.trim().is_empty()
        || invocation.invocation_id.trim().is_empty()
    {
        return Err(AdapterError::rejected(
            "workspace source request contract mismatch",
        ));
    }
    let selection: Selection = serde_json::from_value(invocation.request_payload.clone())
        .map_err(|error| AdapterError::rejected(error.to_string()))?;
    let input = serde_json::to_string(&invocation).map_err(unavailable)?;
    let id = invocation.invocation_id;
    let saved = db
        .query_row(
            "SELECT input,payload FROM source_reads WHERE invocation_id=?1",
            [&id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(unavailable)?;
    let payload = if let Some((previous, payload)) = saved {
        if previous != input {
            return Err(AdapterError::rejected(
                "workspace source request identity conflict",
            ));
        }
        serde_json::from_str::<Value>(&payload).map_err(unavailable)?
    } else {
        let text = scope.source(workspace, &selection.revision, &selection.path)?;
        let payload = json!({"revision":selection.revision,"path":selection.path,"text":text});
        let encoded = serde_json::to_string(&payload).map_err(unavailable)?;
        if encoded.len() > invocation.max_response_bytes {
            return Err(AdapterError::rejected(
                "workspace source response exceeds its byte limit",
            ));
        }
        db.execute(
            "INSERT INTO source_reads VALUES (?1,?2,?3)",
            params![id, input, encoded],
        )
        .map_err(unavailable)?;
        payload
    };
    Ok(RequestResponse {
        external_reference: format!("workspace-source:{id}"),
        invocation_id: id,
        payload,
    })
}

fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(error.to_string())
}
