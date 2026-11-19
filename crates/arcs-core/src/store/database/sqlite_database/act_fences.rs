//! Operational dispatch and acknowledgement guards for immutable reservations.
use super::*;
use crate::adapters::CapabilityRef;
use rusqlite::{Transaction, TransactionBehavior};

impl SqliteArtifactStore {
    pub(crate) fn needs_legacy_act_controllers(
        &self,
        version: &VersionId,
    ) -> Result<bool, StoreError> {
        Ok(self.connection.query_row(
            "SELECT a.sequence <= migration.legacy_through_sequence
             FROM artifact_versions a CROSS JOIN reservation_controller_migration migration
             WHERE a.version_id=?1 AND migration.singleton=1",
            [&version.0],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn register_act_controller(
        &self,
        subject: &SubjectId,
        schema: &SchemaId,
        version: &VersionId,
        controller: &CapabilityRef,
    ) -> Result<bool, StoreError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let current: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM current_artifacts WHERE subject=?1 AND schema_id=?2 AND version_id=?3)",
            params![subject.0, schema.0, version.0], |row| row.get(0),
        )?;
        if !current {
            return Err(StoreError::IdempotencyConflict);
        }
        tx.execute(
            "INSERT OR IGNORE INTO reservation_act_controllers(reservation_version,adapter_id,capability_id) VALUES(?1,?2,?3)",
            params![version.0, controller.adapter_id.0, controller.capability_id.0],
        )?;
        let open: bool = tx.query_row(
            "SELECT closed_at IS NULL FROM reservation_act_controllers
             WHERE reservation_version=?1 AND adapter_id=?2 AND capability_id=?3",
            params![
                version.0,
                controller.adapter_id.0,
                controller.capability_id.0
            ],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(open)
    }

    pub(crate) fn pending_act_controllers(
        &self,
        version: &VersionId,
    ) -> Result<Vec<CapabilityRef>, StoreError> {
        let mut query = self.connection.prepare(
            "SELECT adapter_id,capability_id FROM reservation_act_controllers
             WHERE reservation_version=?1 AND closed_at IS NULL ORDER BY adapter_id,capability_id",
        )?;
        let rows = query.query_map([&version.0], |row| {
            Ok(CapabilityRef::new(
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn confirm_act_controller_closed(
        &self,
        version: &VersionId,
        controller: &CapabilityRef,
    ) -> Result<(), StoreError> {
        if self.connection.execute(
            "UPDATE reservation_act_controllers SET closed_at=COALESCE(closed_at,CURRENT_TIMESTAMP)
             WHERE reservation_version=?1 AND adapter_id=?2 AND capability_id=?3",
            params![
                version.0,
                controller.adapter_id.0,
                controller.capability_id.0
            ],
        )? != 1
        {
            return Err(StoreError::IdempotencyConflict);
        }
        Ok(())
    }
}
