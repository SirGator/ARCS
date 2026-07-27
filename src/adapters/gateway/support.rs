use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::{ArtifactId, VersionId};

static NEXT_GATEWAY_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Prozessweit eindeutige Identität einer Gateway-Instanz.
///
/// Der Typ bleibt vollständig innerhalb des Cores und ist bewusst nicht
/// serialisierbar. Dadurch kann ein Adapter weder eine Gateway-Zuordnung
/// vorgeben noch einen Session-Handle über Prozessgrenzen rekonstruieren.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GatewayInstanceId(u64);

impl GatewayInstanceId {
    /// Reserviert eine Identität, ohne beim Erreichen von `u64::MAX` wieder
    /// bei einer bereits vergebenen ID zu beginnen.
    pub(super) fn allocate() -> Option<Self> {
        NEXT_GATEWAY_INSTANCE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .ok()
            .map(Self)
    }
}

/// Opaquer, an genau eine registrierte Adapterverbindung gebundener Handle.
///
/// Gateway-Identität und Token sind privat. So kann ein normaler
/// Bibliotheksnutzer weder die ID einer anderen Installation einsetzen noch
/// einen zufällig gleichlautenden Token eines anderen Gateways wiederverwenden.
/// Ein späterer Prozess-Transport erzeugt diesen Handle erst nach echter
/// Authentifizierung der Verbindung.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct AdapterSession {
    pub(super) gateway_instance_id: GatewayInstanceId,
    pub(super) token: u64,
}

/// Vom Core erzeugtes Identitätspaar für eine neue unveränderliche Version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifactIds {
    pub artifact_id: ArtifactId,
    pub version_id: VersionId,
}

/// Injizierbare ID-Quelle, damit Adapter niemals ihre eigenen IDs bestimmen.
pub trait ArtifactIdGenerator: Send {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds;
}

/// Einfache monotone ID-Quelle für einen einzelnen Runtime-Knoten.
///
/// Dauerhafte Multi-Node-Installationen sollten denselben Port mit einer
/// persistenten beziehungsweise global eindeutigen Implementierung ersetzen.
pub struct SequenceIdGenerator {
    node_prefix: String,
    next_sequence: u64,
}

impl SequenceIdGenerator {
    pub fn new(node_prefix: impl Into<String>) -> Self {
        Self {
            node_prefix: node_prefix.into(),
            next_sequence: 1,
        }
    }
}

impl ArtifactIdGenerator for SequenceIdGenerator {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let artifact_id = format!("{}-{artifact_type}-{sequence}", self.node_prefix);
        GeneratedArtifactIds {
            artifact_id: ArtifactId(artifact_id.clone()),
            version_id: VersionId(format!("{artifact_id}-v1")),
        }
    }
}

/// Injizierbare vertrauenswürdige Zeitquelle.
pub trait Clock: Send + Sync {
    fn now_rfc3339(&self) -> String;
}

/// UTC-Systemzeit ohne zusätzliche Laufzeitabhängigkeit.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_rfc3339(&self) -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format_unix_timestamp(seconds)
    }
}

fn format_unix_timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (year, month, day) = civil_date_from_unix_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnants kalenderarithmetische Umrechnung. Sie arbeitet rein
// ganzzahlig und ist für alle vom u64-Unixzeitstempel erreichbaren Tage stabil.
fn civil_date_from_unix_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_unix_timestamps_as_rfc3339_utc() {
        assert_eq!(format_unix_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_timestamp(946_684_800), "2000-01-01T00:00:00Z");
        assert_eq!(format_unix_timestamp(1_720_396_800), "2024-07-08T00:00:00Z");
    }
}
