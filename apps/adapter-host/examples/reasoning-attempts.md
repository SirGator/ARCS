# Nachvollziehbare Reasoning-Versuche

Dieser Slice ergänzt die Erfahrungsbelege aus `ARCHITECTURE.md`, v3 §9.2/§9.3:
Der Core speichert abgeschlossene Reasoning-V2-Versuche als
`arcs.reasoning_attempt.v1` im vorhandenen Artifact-Netz. Eine gültige Antwort,
eine Antwort ohne Lösung und ein technischer Fehler bleiben unterscheidbar.

## Startbares Beispiel

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/reasoning-attempts/host.json \
  --runtime apps/adapter-host/config/reasoning-attempts/runtime.json \
  --ticks 12
./target/debug/arcs-host \
  --config apps/adapter-host/config/reasoning-attempts/host.json \
  --reasoning-attempts apps/adapter-host/config/reasoning-attempts/query.json
```

Die Eingabe liefert eine Zahlenliste und eine leere Liste. Zwei vorhandene
Reasoner bearbeiten beide Aufgaben über getrennte externe Prozesse. In der
Historie stehen vier Versuche: zwei gültige Antworten mit Kandidaten und zwei
gültige Antworten mit `status: "no_solution"`. Entsprechend enthält
`page_counts` vier `valid_responses`, zwei `solved_candidate_responses` und null
`failures`. Die Daten liegen getrennt unter `var/reasoning-attempts/`.

Wiederholte Historienabfragen und der erneute Runtime-Start erhalten dieselben
Belegversionen. Cache-Replay erzeugt keinen weiteren Versuch. Der mathematische
Solver berechnet den nächsten Listenwert; die Modell-Fixture liefert lediglich
den ersten Wert und führt keine Modellinferenz aus. Ein Kandidat oder dessen
Status `solved` bestätigt keine erreichte Aufgabe. Beleg und Seite tragen
deshalb `candidate_quality_verified: false`.

## Belege und Fehler

Jeder Beleg bindet Capability, Methode, Budget und Request an die exakte
unveränderliche Version des Dispatch-Zustands. Bei einer gültigen Antwort
referenziert er zusätzlich das geprüfte Antwortartefakt und enthält Status,
Kandidatenzahl und gegebenenfalls den vom Adapter gemeldeten Verbrauch.

Fehlgeschlagene Versuche enthalten `outcome.kind: "failed"`, eine typisierte
Fehlerklasse und die bestehende Core-Entscheidung `retryable`. Klassen sind
`adapter_timeout`, `adapter_unavailable`, `adapter_rejected`, `input_data`,
`authorization`, `invalid_response`, `core` und `contract`. Diese Zuordnung
entsteht aus dem tatsächlichen Fehlertyp im Core. Ein Text in einer
Adapterfehlermeldung kann sie nicht bestimmen. `retryable` beschreibt nur die
bisherige technische Wiederholbarkeit; es startet keinen neuen Aufruf.

Antwort beziehungsweise Fehlerbeleg und abschließender Invocation-Zustand
committen gemeinsam einschließlich ihrer Relationen. Die Belege verweisen
auch auf die ursprünglichen Kontextversionen. Eine später ausdrücklich
veranlasste Wiederholung erhält eine eigene Dispatch-Version und einen neuen
Beleg; frühere Fehlschläge bleiben erhalten. Die Leseprüfung kontrolliert die
gespeicherten Versionen und die Verknüpfung mit dem abschließenden Zustand.
Der Publisher `arcs.reasoning.attempt` ist für den Core reserviert.

`host_completion_latency_ms` misst die Zeit von der vorbereiteten Ausführung
bis zum Beginn der Antwortübernahme im Core. Datenabruf, Adapter-I/O und eine
verzögerte Abholung können dazugehören. Das ist keine reine Modellrechenzeit.
`adapter_reported_iterations` und `adapter_reported_output_tokens` bleiben
Adapterangaben; sie belegen weder tatsächliche Kosten noch geprüfte Qualität.

## Begrenzte Betreiberabfrage

Die JSON-Datei benennt ausdrücklich die zu lesenden Capabilities:

```json
{
  "capabilities": [
    {"adapter_id": "example.solver", "capability_id": "solution.propose"},
    {"adapter_id": "example.model-fixture", "capability_id": "solution.propose"}
  ],
  "after_sequence": 0,
  "limit": 16
}
```

Erlaubt sind eine bis 16 verschiedene aktuell freigegebene Reason-Capabilities
und eine Seitengröße von eins bis 256. Die Eingabedatei ist auf 65536 Bytes
begrenzt und wird vor dem Hoststart geprüft. Die CLI öffnet und registriert
die konfigurierten Adapter samt Metadaten-Handshake. Sie startet dabei keine
TaskRuntime, holt keine Eingaben ab und führt keine konfigurierten Reasoning-,
Request- oder Action-Aufträge aus. Die eingebettete API ist
`Host::reasoning_attempts(&AttemptQuery)`.

`through_sequence` begrenzt die Abfrage auf den zu Beginn sichtbaren Stand.
Pro Capability werden höchstens `limit + 1` Zeilen über einen Index gesucht;
die zusammengeführte Seite verarbeitet höchstens `limit` Zeilen. Belege und
ausgewählte Elternmetadaten sind jeweils auf 32 KiB begrenzt. Große Kontext-
und Ergebnisfelder werden in Rust nicht für die Historie materialisiert;
SQLite muss die gespeicherten JSON-Dokumente weiterhin lesen und auswerten.
Das ist eine Mengenbegrenzung, keine harte Laufzeitgarantie.

`next_sequence` wird als `after_sequence` der nächsten Abfrage verwendet.
`has_more` zeigt weitere Zeilen innerhalb dieses Stands an. Auch übersprungene
unvertrauenswürdige oder zu große Zeilen bewegen den Cursor; `scanned`,
`skipped_untrusted` und `skipped_oversized` machen dies sichtbar. Eine Seite
kann deshalb leer sein und trotzdem weitere Zeilen haben. Inkonsistente
Core-Belege führen ausdrücklich zu einem Fehler. `page_counts` zählt nur die
gültigen Belege dieser Seite, keine globale Erfolgsrate.

Die Historie enthält keine Kontextpayloads, Kandidateninhalte oder rohen
Adapterfehlermeldungen. Sie enthält Korrelations-IDs, Budgets und Capability-
Referenzen und ist für den berechtigten Betreiber bestimmt. Die Freigabe aller
angefragten Capabilities wird vor dem Lesen geprüft.

## Abdeckung und nächste Arbeiten

`coverage: "recorded_completed_v2_attempts"` bezeichnet die tatsächlich im
neuen Pfad abgeschlossenen Versuche. Ältere V1-/V2-Aufrufe werden nicht
nachträglich als gemessene Versuche ausgegeben. Ein unbekannter Ausgang nach
Absturz, ein verworfener später Worker oder ein noch laufender Aufruf erhält
keinen erfundenen Ergebnisbeleg. Ein persistiertes Ergebnis bleibt auch nach
einem verlorenen Queue-Checkpoint oder während einer Aufgabenpause lesbar.

```bash
cargo test -p arcs-core --lib --locked --offline reasoning::attempts
cargo test -p arcs-host --test reasoning_attempts --locked --offline
```

Die Tests prüfen gültige Antworten, `no_solution`, technische und ungültige
Antworten, explizite Wiederholung, Rollback, aktuelle Grants, Cursor, Neustarts,
pausierte Worker und verlorene Queue-Checkpoints. Die optionale
[begrenzte Recovery](reasoning-recovery.md) verwendet diese Fehlerbelege für
ausdrücklich erlaubte Alternativen. Allgemeine Neuplanung, gemessene
Aufgabenqualität und Kostenoptimierung bleiben weitere Arbeit. Die dauerhafte
Reasoner-Auswahl bleibt als ursprüngliche Bindung erhalten.
