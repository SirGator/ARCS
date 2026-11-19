//! The simulator advances its fence in the same transaction as the effect.
use arcs_adapter_protocol::ActFence;
use rusqlite::{OptionalExtension, Transaction, params};

use super::error;

pub(super) fn bind(transaction: &Transaction<'_>, resource: Option<&str>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS act_fence (
            singleton INTEGER PRIMARY KEY CHECK (singleton=1),
            resource TEXT NOT NULL,
            authority TEXT,
            revision INTEGER NOT NULL DEFAULT 0,
            version_id TEXT,
            closed INTEGER NOT NULL DEFAULT 0 CHECK (closed IN (0,1)),
            CHECK ((authority IS NULL AND revision=0 AND version_id IS NULL)
                OR (authority IS NOT NULL AND revision>0 AND version_id IS NOT NULL))
        );",
        )
        .map_err(error)?;
    let has_closed: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('act_fence') WHERE name='closed')",
            [],
            |row| row.get(0),
        )
        .map_err(error)?;
    if !has_closed {
        transaction.execute_batch(
            "ALTER TABLE act_fence ADD COLUMN closed INTEGER NOT NULL DEFAULT 0 CHECK (closed IN (0,1));",
        ).map_err(error)?;
    }
    let bound: Option<String> = transaction
        .query_row(
            "SELECT resource FROM act_fence WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(error)?;
    match (bound.as_deref(), resource) {
        (None, Some(resource)) => {
            transaction
                .execute(
                    "INSERT INTO act_fence(singleton,resource) VALUES(1,?1)",
                    [resource],
                )
                .map_err(error)?;
        }
        (left, right) if left == right => {}
        _ => return Err("adapter database is bound to a different fencing configuration".into()),
    }
    Ok(())
}

pub(super) fn accept(
    transaction: &Transaction<'_>,
    resource: Option<&str>,
    fence: Option<&ActFence>,
) -> Result<(), String> {
    let fence = match (resource, fence) {
        (None, None) => {
            let bound: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM act_fence WHERE singleton=1)",
                    [],
                    |row| row.get(0),
                )
                .map_err(error)?;
            return if bound {
                Err("adapter database now requires a reservation fence".into())
            } else {
                Ok(())
            };
        }
        (Some(resource), Some(fence)) if resource == fence.resource && fence.is_valid() => fence,
        _ => return Err("action requires the configured reservation fence".into()),
    };
    let (highest, closed) = check(transaction, fence)?;
    if fence.revision < highest || (fence.revision == highest && closed) {
        return Err("stale or closed reservation fence".into());
    }
    if fence.revision > highest {
        save(transaction, fence, false)?;
    }
    Ok(())
}

pub(super) fn close(
    transaction: &Transaction<'_>,
    resource: Option<&str>,
    fence: &ActFence,
) -> Result<(), String> {
    if resource != Some(fence.resource.as_str()) || !fence.is_valid() {
        return Err("controller fence contract mismatch".into());
    }
    let (highest, closed) = check(transaction, fence)?;
    if fence.revision > highest || (fence.revision == highest && !closed) {
        save(transaction, fence, true)?;
    }
    Ok(())
}

fn check(transaction: &Transaction<'_>, fence: &ActFence) -> Result<(u64, bool), String> {
    let (authority, highest, version, closed): (Option<String>, i64, Option<String>, bool) =
        transaction
            .query_row(
                "SELECT authority,revision,version_id,closed FROM act_fence WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(error)?;
    if authority.as_ref().is_some_and(|id| *id != fence.authority)
        || (fence.revision == highest as u64
            && version.as_ref() != Some(&fence.reservation_version_id.0))
    {
        return Err("conflicting or foreign reservation fence".into());
    }
    Ok((highest as u64, closed))
}

fn save(transaction: &Transaction<'_>, fence: &ActFence, closed: bool) -> Result<(), String> {
    transaction.execute(
        "UPDATE act_fence SET authority=?1,revision=?2,version_id=?3,closed=?4 WHERE singleton=1",
        params![fence.authority, fence.revision as i64, fence.reservation_version_id.0, closed],
    ).map_err(error)?;
    Ok(())
}
