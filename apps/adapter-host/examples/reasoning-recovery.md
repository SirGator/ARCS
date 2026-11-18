# Begrenzter Reasoning-Fallback

Eine Route kann nach einem belegten technischen Ausfall zu ausdrücklich
konfigurierten Alternativen wechseln. Grundlage sind die
[Core-Belege zu Reasoning-Versuchen](reasoning-attempts.md). Jeder Wechsel
erhält eine dauerhafte Entscheidung im vorhandenen Artifact-Netz.

## Startbares Beispiel mit echtem Timeout

```bash
cargo build --workspace --bins --locked --offline
cargo run -p arcs-host --example reasoning-recovery --locked --offline -- \
  apps/adapter-host/config/reasoning-recovery/host.json \
  apps/adapter-host/config/reasoning-recovery/runtime.json \
  var/reasoning-recovery/solver.sqlite
```

Das Rust-Beispiel öffnet den normalen Host mit den vorhandenen externen
Solver- und Modell-Fixture-Prozessen. Nach deren Metadaten-Handshake sperrt es
die separate SQLite-Datei des Beispielsolvers. Sein tatsächlicher Aufruf
überschreitet dadurch das Laufzeitbudget. Der Core speichert den Timeout,
bevor die erlaubte Modell-Fixture dieselbe ausgewählte Eingabe erhält.
Ein unabhängiger Auftrag kann währenddessen weiterlaufen.

Nach 30 Ticks zeigt `runtime.ready_for_review` zwei Ergebnisse. Unter `history`
stehen ein fehlgeschlagener Solver-Versuch und zwei gültige Modell-Antworten.
`reasoning_recoveries` zeigt Versuch 2 von höchstens 2 mit
`example.model-fixture` als tatsächlicher Auswahl. Die Daten liegen getrennt
unter `var/reasoning-recovery/`. Die Modell-Fixture liefert nur den ersten
Listenwert und führt keine Modellinferenz aus; die Kandidatenqualität wird
weiterhin nicht als geprüft ausgegeben.

Ein normaler CLI-Neustart übernimmt dieselben Ergebnisse und Belege:

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/reasoning-recovery/host.json \
  --runtime apps/adapter-host/config/reasoning-recovery/runtime.json --ticks 4
./target/debug/arcs-host \
  --config apps/adapter-host/config/reasoning-recovery/host.json \
  --reasoning-attempts apps/adapter-host/config/reasoning-recovery/query.json
```

## Betreiberpolicy

Das optionale Feld einer Reasoning-Route ist beispielsweise:

```json
{
  "reasoning_recovery": {
    "alternatives": [{
      "capability": {
        "adapter_id": "example.model-fixture",
        "capability_id": "solution.propose"
      },
      "method": "learned",
      "budget": {
        "max_context_items": 1,
        "max_context_bytes": 8192,
        "max_output_bytes": 4096,
        "max_candidates": 1,
        "max_runtime_ms": 1000,
        "max_iterations": null,
        "max_output_tokens": null
      }
    }],
    "delay_ms": 50
  }
}
```

Zulässig sind eine bis sieben geordnete Alternativen und eine Verzögerung von
0 bis 60000 Millisekunden. Zusammen mit dem ersten Request gibt es höchstens
acht logische Aufrufversuche pro Anfrage. Schema, Rolle, Methode und Budgets
aller Optionen werden gegen die aktuellen Grants geprüft. Wiederholte
Capability-/Methodenpaare sind erlaubt und verbrauchen jeweils einen Eintrag.
Routen ohne Policy behalten ihr bisheriges Verhalten und ihre gespeicherte
Identität.

Nur ein gespeicherter Fehler vom Typ `adapter_timeout` oder
`adapter_unavailable` mit `retryable: true` erlaubt den nächsten Eintrag.
Ungültige Antworten, `no_solution`, Budget-Antworten, Ablehnungen,
Autorisierungsfehler und Fehler beim Abruf der Eingabedaten lösen keinen
Fallback aus. Ein Fehler ohne passenden Core-Abschlussbeleg berechtigt
ebenfalls keinen Wechsel. Nach Verbrauch der Liste endet die Recovery.

Ein Fehler gilt zunächst für diesen Request. Neue Aufgaben und Goal-Runden
können denselben Erst-Reasoner anfragen. Ist dessen Session weiterhin
quarantänisiert, prüft der Core die neue Anfrage und dokumentiert ihre lokale
Ablehnung als `adapter_unavailable`. Dabei werden weder Binärquellen gelesen
noch Daten an die alte Session geschickt oder Workerplätze belegt. Anschließend
kann die erlaubte Alternative übernehmen. Ein solcher Core-Aufrufversuch
beweist keine Zustellung oder Modellberechnung beim Backend. Die getrennte,
optionale [Wiederverbindungs-Policy](reasoner-reconnect.md) kann reine Reason-
Prozesse neu verbinden. Während ihrer begrenzten Versuche kann neue Arbeit
warten; ohne diese Policy bleiben quarantänisierte Sessions lokal gesperrt.

## Persistenz und Zuständigkeiten

`arcs.reasoning_recovery.v1` bindet die ursprüngliche Anfrage, ausgewählte
Kontextversionen, Binärquellenauswahl, Betreiberpolicy und Aufgabe. Eine
Definition ist auf 60000 Bytes begrenzt. `arcs.reasoning_recovery_step.v1`
verknüpft jeden Eintrag mit Definition, Vorgänger, exaktem Fehlerbeleg und
Fälligkeit. Konkurrierende Schreiber einigen sich auf den zuerst gespeicherten
Beleg samt Fälligkeit. Der Publisher `arcs.reasoning.recovery` ist für den Core
reserviert.

Die Fälligkeit wird bei der ersten Verarbeitung des Fehlers festgeschrieben.
Der Scheduler prüft sie mit der vertrauenswürdigen Hostzeit beziehungsweise
`tick_at`. Ein Neustart setzt sie nicht neu. Ein zurückspringender Zeitwert
kann die Wartezeit verlängern. Wartezeit und belegte Adapter belegen keine
neuen Worker. Pro Tick und ausgewähltem Task wird höchstens ein neuer Versuch
ausgeführt.

Der neue Request erhält seine eigene Identität, Capability, Methode und Budget.
Objektive, Kontextauswahl, Binärreferenzen, Zielschema, erlaubte Handlungen und
sonstige Constraints bleiben gebunden. Eine Referenz auf den Recovery-Schritt
gelangt in die Anfrage; zusätzliche private Policy- oder Suchdaten werden
dadurch nicht exportiert. Der Core prüft den vollständigen Request und die
Fehlerkette, bevor er einen Aufruf zulässt. Ein abgeschlossener Fehlversuch
kann nicht unter derselben Schrittidentität erneut gestartet werden.

Die ursprüngliche [Reasoner-Auswahl](reasoner-routing.md) bleibt unverändert
als historische Erstbindung erhalten. Beim Fallback prüft der Core diese
Bindung an der ursprünglichen Anfrage und die tatsächliche Alternative an der
Recovery-Policy. Joins, erzeugte Pläne und Goal-Verification prüfen den
tatsächlichen Ergebnisproduzenten anhand dieser Belege. Ein beliebiger anderer
Reasoner aus dem Katalog reicht dafür nicht aus.

CLI und Monitor zeigen unter `reasoning_recoveries` höchstens 50 zuletzt
angelegte Aufgaben mit Recovery: Definition, aktuellen Schritt, Versuchsnummer,
Limit, Fälligkeit, Auswahl, vorangegangenen Fehler und optionalen Invocation-
Status. Private Constraints und Kontextpayloads werden dort nicht ausgegeben.
Eine geplante Alternative kann noch ohne Invocation-Status sein. Der
Index im bestehenden Aufgabenjournal speichert nur Core-Versionsreferenzen;
die Entscheidung selbst bleibt im Artifact-Netz.

## Goals, Pläne und Budgets

Eine Goal-Policy gilt für die Planung, unabhängig von ihrem Assessor. Jede
Runde erhält eine eigene Recovery-Kette. Eine Aufgabenpause übernimmt weiterhin
fertige Worker-Antworten in den Core, wählt oder startet während der Pause aber
keine Alternative. Nach dem Fortsetzen wird der Abschlussbeleg ausgewertet.
Fristen und Planabbruch sperren noch nicht gestartete Alternativen.

Neue Kandidaten durchlaufen Verification und die konfigurierte Approval.
Die Prüfung bindet auch die genaue Goal-Runde. Bereits erteilte Freigaben
werden nicht auf andere Kandidaten übertragen. Die Recovery wiederholt keine
Act-, Request- oder Output-Operation. Laufende exklusive Goal-Reservierungen
bleiben gemäß den bestehenden Kontrollregeln erhalten.

Ein erzeugter Plan berücksichtigt das höchste zulässige Erstbudget plus die
Summe aller Recovery-Budgets pro maximaler Goal-Runde. Assessment und Actions
werden weiterhin separat gezählt. Fehlende erforderliche Tokenlimits und
Überläufe werden abgewiesen. Die Warteverzögerung ist kein Rechenbudget;
`max_lifetime_ms` begrenzt weiterhin die gesamte Lebensdauer einschließlich
Wartephasen. Binär verarbeitende Goal-Alternativen brauchen eigene passende
Receiver-Grants. Auch ein kleineres alternatives Kontextbudget bleibt wirksam;
unpassender Kontext berechtigt keinen weiteren stillen Wechsel.

## Prüfung und Grenzen

```bash
cargo test -p arcs-core --lib --locked --offline reasoning::recovery
cargo test -p arcs-host --test reasoning_recovery --locked --offline
```

Die Tests prüfen echte Adapterwechsel, aktuelle Grants, Policy- und
Requestmanipulation, begrenzte Ketten, konkurrierende Schreiber, unbekannte
Ausgänge, dauerhafte Verzögerungen, verlorene Checkpoints, Aufgabenpausen,
Fristen, Planabbruch, tatsächliche Ergebnisproduzenten und manuelle Freigaben
über mehrere Goal-Runden. Kandidaten einer anderen Runde werden abgewiesen.

Ein unbekannter Ausgang nach Absturz erhält keinen erfundenen alten
Fehlerbeleg. Seine Wiederaufnahme benutzt dieselbe Anfrageidentität über den
bisherigen Core-Recovery-Pfad. Die Zahl tatsächlicher Backend-Berechnungen
bei verlorenen Antworten hängt weiterhin von der Deduplizierung des Adapters
ab. Die neue Grenze zählt logische Requests mit bekannten Abschlüssen,
einschließlich lokaler Ablehnungen.

Allgemeine Wiederverbindung anderer Rollen, gelernte Auswahl nach Kosten oder Qualität,
Modellvergleich, allgemeine Neuplanung und Fallback bei semantisch schlechten
Kandidaten bleiben weitere Arbeit. Die universelle Architektur und ihre
Phasenabnahmen sind damit weiterhin nicht vollständig umgesetzt.
