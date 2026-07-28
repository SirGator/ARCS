use std::sync::atomic::{AtomicU64, Ordering};

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
