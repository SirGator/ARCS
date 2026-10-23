# ARCS Adapter SDK

Gemeinsame Rust-Basis für externe Adapter. Das Paket hängt vom
`arcs-adapter-protocol` und Serde JSON ab und enthält weder Core- noch
Geräteabhängigkeiten. Der Host kann auch Adapter in anderen Sprachen
anbinden, sofern sie das gleiche Protokoll implementieren.

Ein Adapter implementiert den Trait `Adapter`. Er liefert sein Manifest
und überschreibt die benötigten Methoden:

| Methode | Verantwortung des Treibers |
| --- | --- |
| `manifest` | Stabile Identität, Capabilities, Schemas und benötigte Permissions beschreiben |
| `poll` | Eine ausstehende Observation anbieten; ohne Daten `None` liefern |
| `acknowledge` | Eine bekannte Observation nach dem Host-Commit wiederholbar bestätigen |
| `request` | Eine gezielte Datenabfrage beantworten |
| `transform` | Einen geprüften Payload deterministisch in einen Ergebnis-Payload übersetzen |
| `output` | Einen bereits autorisierten Ausgabeauftrag dauerhaft idempotent zustellen |
| `execute` | Einen autorisierten Auftrag fachlich prüfen und idempotent ausführen |
| `read_data` | Unveränderliche referenzierte Daten als begrenzten Block liefern |

`serve` übernimmt das begrenzte JSON-Lines-Framing, den Hello-Dialog, die
Korrelation jeder Antwort, Dispatch und Shutdown. Ungültige Versionen,
Sessionzustände und Aufrufe einer falschen Adapter-ID oder Capability werden
abgewiesen. Requests werden gegen die angebotenen Eingabe-/Ausgabeschema-IDs
geroutet; bei Actions werden Capability-Typ und Ergebnisschema-ID geprüft.
Die interne Action-Struktur und ihr fachlicher Eingabevertrag bleiben beim
Treiber. JSON-Schema-Payloadvalidierung und Autorisierung erfolgen weiterhin
im Host/Core. Das SDK prüft zusätzlich Antwortkorrelation, Request-Payloadlimit
und die Identität und Capability ausgehender Observations.

Eine neue Implementierung verwendet im ausführbaren Programm etwa:

```rust,no_run
use std::io::{self, BufReader};
use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor,
    CapabilityId, SchemaId,
};
use arcs_adapter_sdk::{Adapter, serve};

struct Driver;

impl Adapter for Driver {
    fn manifest(&self) -> AdapterManifest {
        AdapterManifest {
            protocol_version: 1,
            adapter_id: AdapterId("example.device".into()),
            adapter_version: "1.0.0".into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("state.observe".into()),
                contract: CapabilityContract::Observe {
                    emits: vec![SchemaId("example.state.v1".into())],
                },
                required_permissions: vec!["device.read".into()],
            }],
        }
    }
    // poll und acknowledge ergänzen, sobald Beobachtungen vorliegen.
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    serve(&mut Driver, &mut input, &mut output)?;
    Ok(())
}
```

`poll` liefert standardmäßig keine Daten. Nicht implementierte Request-, Transform-,
Output-, Action-, Datenabruf- und ACK-Methoden liefern `unsupported_operation`. Ein Adapter soll
nur tatsächlich unterstützte Capabilities anbieten. Logs gehören nach stderr.

`AdapterError::rejected` kennzeichnet einen abgewiesenen Auftrag,
`AdapterError::unavailable` einen vorübergehenden Ausfall. Eigene Codes sind
über die öffentlichen Felder möglich. Fachliche Fehler werden als korrelierte
Protokollantwort gesendet. Fehlerhafte Frames und I/O-Fehler beenden `serve`.
Der vorhandene Host schließt eine Verbindung auch nach einer Fehlerantwort.

Das SDK hält keine dauerhafte Outbox und kein Invocation-Journal vor. Der
Treiber muss unbestätigte Daten und Ergebnisse so speichern, dass Neustarts
und verlorene Antworten keine zweite Wirkung verursachen. Die
Sensorimplementierung verwendet wiederholbare Profildaten; der Lüfter
speichert Zustand, Journal und Outbox atomar in SQLite. Eine erfolgte
Gerätewirkung wird durch eine später fehlerhafte Antwort nicht rückgängig
gemacht. Dafür gelten die Recovery-Verträge des jeweiligen Treibers.

Anwendungsbeispiele liegen in `adapters/simulated-sensor/src/main.rs` und
`adapters/simulated-fan/src/main.rs`. Beide verwenden dieselbe `serve`-Funktion.
Weitere Rollen wie Input und Reasoning sind im Manifestvertrag
beschreibbar, haben aber noch keine Nachrichten und Callbacks in diesem
stdio-SDK.

Für referenzierte Binärdaten prüft das SDK zusätzlich Referenzform,
Capability-Typ, Blockgröße, Offset und Antwortlänge. `read_data` wird für
Observe-, Request- und Transform-Capabilities unterstützt. Die Quelle muss die exakten
Metadaten einer bekannten Referenz prüfen und ihre Bytes unverändert
aufbewahren. Grants und Datenlimits prüft der Host; das SDK vergibt keine
eigenen Leserechte. Ein Beispiel mit persistenten Snapshots liegt in
`adapters/file-data`. Die vollständigen Nachrichten stehen im
[Adapterprotokoll](../../ADAPTER_PROTOCOL.md#referenzierte-daten).

Für einen Transform implementiert der Adapter `transform(TransformInvocation)`.
Das SDK prüft Adapter-ID, Capability-Typ, Eingabe-/Ausgabeschema-IDs und
Antwortkorrelation. Der Adapter validiert seine fachlichen Eingaben und
arbeitet ohne externe Effekte. Alle Parameter müssen in der Eingabe oder
einer eindeutig versionierten Implementierung festgelegt sein. Die gleiche
Invocation muss auch nach Neustarts dasselbe Ergebnis liefern können.
Grants, Trust, Herkunft und die Ergebnis-Persistenz setzt der Host/Core.
Ein vollständiges Beispiel liegt unter `adapters/normalize-number`.

Für eine Ausgabe implementiert der Adapter `output(OutputInvocation)` und
liefert `OutputResponse`. Das SDK prüft den Output-Capability-Typ,
Schema-IDs, Pflichtreferenzen, Korrelation und Resultatlimit. Verification,
Approval und Grants prüft der Core vor der Zustellung. Ein Treiber muss
verlorene Antworten und Prozessneustarts anhand der Invocation-ID erkennen
und darf dieselbe Zustellung nicht erneut wirken lassen. Die Dateiausgabe
unter `adapters/file-output` zeigt ein persistentes Journal samt atomarer
Dateiveröffentlichung und Wiederherstellung zwischen beiden Schritten.
