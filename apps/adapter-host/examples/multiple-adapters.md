# Mehrere Anschlüsse für jede Adapter-Rolle

ARCS verwaltet angeschlossene Komponenten wie eindeutig benannte Installationen.
Eine Installation kann mehrere Rollen anbieten. Jede Rolle kann durch mehrere
Geräte, Datenquellen, Dienste oder Reasoning-Verfahren gleichzeitig vertreten sein.
Der Anschluss ist nicht an eine bestimmte KI-Technologie gebunden.

Die [Beispielkonfiguration](../config/multiple-adapters/host.json) startet elf
unabhängige Prozesse:

| Installationen | Rollen |
| --- | --- |
| Zwei JSON-Eingangsquellen | Input |
| Zwei simulierte Lüfter mit Temperaturquelle | Observe, Request, Act |
| Zwei Zahlen-Normalisierer | Transform |
| Ein allgemeiner JSON-Berichtsdarsteller | Transform |
| Ein mathematischer Solver und ein Modell-Testadapter | Reason |
| Zwei getrennte Dateiausgaben | Output |

Damit ist jede Rolle mindestens zweimal verfügbar. Der Modell-Testadapter
liefert aufgezeichnetes Beispielverhalten und führt keine echte ML-Inferenz aus.

Im ARCS-Workspace:

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/multiple-adapters/host.json \
  --list-adapters
```

Der Katalog enthält nur die angeschlossenen externen Installationen und ihre
freigeschalteten Fähigkeiten: Rolle, akzeptierte und erzeugte Schemas sowie
die erlaubten Reasoning-V2-Verfahren. Er fragt Manifeste ab und führt keinen
Input-/Observe-Poll, keine konfigurierte Abfrage und keine Aktion aus.
Die Adapter können beim Start ihre eigene Installation und Journale anlegen.
Der Katalog ist eine Beschreibung der Anschlusskonfiguration, kein laufender
Health-Check und keine Ausführungsberechtigung.

Mit der Host-API lässt sich nach Rollen auswählen:

```rust,ignore
let connections = host.adapter_catalog();
let reasoners = host.capabilities(arcs::adapters::AdapterRole::Reason);
// Enthält beide Ziele; die Anwendung wählt ausdrücklich eine CapabilityRef.
let selected = arcs::adapters::CapabilityRef::new(
    "multi.reasoner.solver", "solution.propose",
);
```

`AdapterId` bezeichnet eine Installation, `CapabilityId` eine lokale Fähigkeit.
`multi.fan.left/fan.set` und `multi.fan.right/fan.set` sind deshalb unabhängige
Ziele. Gleiche lokale Capability-, Geräte-, Ereignis- oder Tasknamen dürfen bei
unterschiedlichen Installationen vorkommen. Doppelte Adapter-IDs werden vor
dem Start abgewiesen. Die Core-Subjects und Invocation-Identitäten trennen
die Quellen; es gibt keine automatische Wahl der ersten passenden Rolle.

Profile mit einer `adapter_id` erhalten pro Installation einen eigenen Wert.
Lüfter und Normalisierer unterstützen dafür zusätzlich:

```bash
arcs-simulated-fan <profile.json> <journal.sqlite> --adapter-id fan.left
arcs-normalize-number --adapter-id normalize.left
```

Die bisherigen Aufrufe behalten ihre Standard-IDs. Zustandsbehaftete
Installationen benötigen getrennte Journale. Der Lüfter bindet seine
Adapter-ID dauerhaft an sein Journal. Bestehende Journale ohne ID-Bindung
gehören weiterhin zur bisherigen Standardinstallation `demo.fan` und können
nicht als neue benannte Installation übernommen werden.

Die gemeinsame Host-Konfiguration steht unter `var/multiple-adapters/core.sqlite`;
Adapterjournale und getrennte Ausgabeordner liegen daneben. Ein normaler
Host-Lauf ohne `--list-adapters` verarbeitet Inputs und Beobachtungen sowie
die konfigurierten Abfragen und Transforms. Reasoning-Aufrufe und freigegebene
Effekte werden ausdrücklich durch die Anwendung veranlasst.

Die Prozess-Tests verbinden alle elf Programme. Sie prüfen beide Eingangsquellen
mit beiden Reasonern, gezielte Requests und Transforms, getrennt freigegebene
Actions und Outputs, falsch ausgewählte Empfänger, identische lokale Namen
sowie Wiederholung nach Neustart ohne zweite Wirkung. Die Test-Freigabepolicy
bleibt ausschließlich im Test.

Anschlüsse werden derzeit beim Host-Start konfiguriert; automatisches Erkennen
und An-/Abstecken während eines laufenden Hosts sind noch nicht implementiert.
Aufrufe bleiben seriell. Die spätere Orchestrierung kann aus diesem Katalog
mehrere Verfahren auswählen, verketten oder deren Ergebnisse vergleichen.
