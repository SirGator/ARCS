//! Persistente Zugehörigkeit einer Aufgabe zu genau einer Runtime.
//!
//! Die Lease beantwortet die Frage, wer eine Aufgabe gerade bearbeitet. Sie
//! lebt im Core und nicht im Journal, damit sie einen Neustart und einen
//! Knotenwechsel überlebt. Das Fencing-Token macht den Besitzwechsel
//! eindeutig: Ein zurückgebliebener Eigentümer kann nach der Übergabe nicht
//! mehr schreiben.

use arcs::core::SystemClock;
use arcs::runtime::ownership::{OwnershipError, OwnershipService, TaskLease};

use crate::task_runtime::TaskRuntime;
use crate::{Host, HostError};

/// Dauer der Aufgaben-Lease. Sie deckt eine Arbeitsrunde ab und ist bewusst
/// deutlich länger als ein einzelner Task-Schritt.
pub const TASK_LEASE_SECONDS: u64 = 900;

/// Gemeinsame Uhr der Leases. Der Core braucht keine eigene Zeitquelle.
static CLOCK: SystemClock = SystemClock;

/// Stabiler Schlüssel einer Aufgabe im Core.
pub fn task_key(sequence: u64) -> String {
    format!("goal:{sequence}")
}

impl TaskRuntime {
    /// Sichert oder erneuert die Zugehörigkeit der Aufgabe zu dieser Runtime.
    ///
    /// Der Aufruf ist für denselben Eigentümer wiederholbar: Eine vorhandene
    /// Lease wird verlängert, ohne das Token zu verändern. Ein fremder
    /// Eigentümer erhält die Ablehnung, solange die Lease wirksam ist.
    pub(super) fn hold_task_lease(
        &self,
        host: &Host,
        sequence: u64,
    ) -> Result<TaskLease, HostError> {
        let service = OwnershipService::new(host.store(), host.schemas(), &CLOCK);
        let key = task_key(sequence);
        let owner = self.config.runtime_id.clone();
        if let Some(current) = service.current(&key).map_err(HostError::Ownership)?
            && current.owner_id == owner
        {
            match service.renew(&current, &owner, TASK_LEASE_SECONDS) {
                Ok(lease) => return Ok(lease),
                // Eine abgelaufene Lease wird neu zugeteilt; ein fremdes
                // Token bedeutet einen fremden Eigentümer.
                Err(OwnershipError::LeaseExpired { .. })
                | Err(OwnershipError::FencingTokenMismatch { .. }) => {}
                Err(error) => return Err(HostError::Ownership(error)),
            }
        }
        service
            .acquire(&key, &owner, TASK_LEASE_SECONDS)
            .map_err(HostError::Ownership)
    }

    /// Prüft, ob diese Runtime die Aufgabe weiterhin exklusiv hält.
    ///
    /// Ohne Lease ist die Aufgabe frei: Die Prüfung schlägt erst fehl, wenn
    /// ein anderer Eigentümer die Aufgabe übernommen hat.
    pub(super) fn check_task_lease(&self, host: &Host, sequence: u64) -> Result<(), HostError> {
        let service = OwnershipService::new(host.store(), host.schemas(), &CLOCK);
        let key = task_key(sequence);
        let Some(lease) = service.current(&key).map_err(HostError::Ownership)? else {
            return Ok(());
        };
        if lease.owner_id != self.config.runtime_id {
            return Err(HostError::Ownership(OwnershipError::NotOwner {
                expected: lease.owner_id,
                actual: self.config.runtime_id.clone(),
            }));
        }
        service.validate(&lease).map_err(HostError::Ownership)
    }
}
