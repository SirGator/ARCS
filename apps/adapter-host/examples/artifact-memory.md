# Gedächtnisabruf aus dem Artifact-Netz

Die Aufgaben-Runtime kann ihren Pflichtkontext um passende gespeicherte
Artefakte ergänzen. Grundlage bleiben derselbe Store und dieselben Beziehungen.
Es gibt keine zusätzliche Gedächtnisdatenbank und keinen weiteren Agenten.

Der erste Abruf kombiniert explizite Versionen, Subjects, Tags, Schemas und
begrenzte Graphsuche. Er verwendet noch keine Sprachähnlichkeit, Embeddings oder
automatische Konsolidierung. Die Relevanz folgt einer Betreiberpolicy.
Die optionale [Textsuche](text-memory.md) ergänzt diesen Abruf um exakte
Suchwörter in freigegebenen Stringfeldern und einen wiederaufbaubaren Index.

## Ausführbares Beispiel

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/memory/host.json \
  --runtime apps/adapter-host/config/memory/runtime.json \
  --ticks 20
```

Zwei echte JSON-Input-Prozesse liefern Projektnotizen und eine Zahlenaufgabe.
Der externe Solver erhält die Zahlenaufgabe und das ausgewählte `body` einer
Notiz des freigegebenen Projekts. Das Feld `private_note` und die Notiz des
anderen Projekts verlassen den Store nicht. Journale liegen getrennt unter
`var/memory/`.

Der Solver löst weiterhin seine deterministische Zahlenaufgabe. Er zeigt damit
den Transport und die Kontextgrenzen; ein Vorteil durch semantische Nutzung
der Notiz wird mit diesem Beispiel nicht behauptet. Der Bericht enthält ein
Ergebnis zur Prüfung. Ein erneuter Start wiederholt die fertige Aufgabe nicht.

## Option `memory` einer Reasoning-Route

```json
{
  "memory": {
    "scopes": [
      {
        "filter": {
          "schema_id": "arcs.note.project_memory.v1",
          "subject": "input:12:memory.input:14:memory.capture:15:project-nearest"
        },
        "payload_fields": ["body"]
      }
    ],
    "relations": ["based_on", "supported_by", "part_of_goal"],
    "search": {
      "max_depth": 2,
      "max_visited": 64,
      "max_edges": 128,
      "max_candidates": 16
    },
    "max_items": 2,
    "max_bytes": 2048
  }
}
```

Subjects sind die vom Core erzeugten, nach Adapter und Capability getrennten
Identitäten. Ein externer Subject-Text wird vom Input-Dienst entsprechend
eingefasst. Das Beispiel verwendet deshalb die gespeicherte Core-Subject-ID.
Die gleiche Source kann Notizen mehrerer Projekte liefern, ohne dass diese
dadurch automatisch gemeinsam freigegeben werden.

Ein Filter erlaubt genau ein Schema und optional ein Subject und Tags.
Alle angegebenen Tags müssen vorhanden sein. Ein Subject oder Tags erlauben
zusätzlich die Suche nach Einstiegspunkten in der Historie. Ein Schemafilter
allein liefert ausschließlich Artefakte, die von den expliziten Startversionen
über erlaubte Beziehungen erreichbar sind. Er durchsucht nicht automatisch
sämtliche Projekte mit diesem Schema.

Startversionen sind der Pflichtkontext des jeweiligen Aufrufs und optionale
`anchors`. Relationen werden in beide Richtungen verfolgt; ihre ursprüngliche
Richtung bleibt im Suchpfad erhalten. Zyklen werden nicht mehrfach besucht.
Die Rangfolge bevorzugt kurze Pfade und danach neuere Core-Commits. Das ist
keine Bewertung fachlicher Wahrheit und keine Änderung vorhandener Netzgewichte.

| Grenze | Bedeutung |
| --- | --- |
| `search.max_depth` | 0–8 Beziehungsschritte |
| `search.max_visited` | 1–2048 besuchte Versionen |
| `search.max_edges` | 1–8192 gelesene Beziehungseinträge, einschließlich erneut gelesener Rückkanten |
| `search.max_candidates` | 1–256 Kandidaten vor der Kontextauswahl |
| `max_items` | 1–64 zusätzliche Kontextstücke |
| `max_bytes` | 128–1.048.576 Bytes für die serialisierten zusätzlichen Kontextstücke |

Große einzelne Artefakte über 256 KiB werden bei diesem Abruf ausgelassen.
Binärdaten werden weiterhin über den getrennt autorisierten Datenvertrag
übertragen. Der Abruf lädt keine großen Bildinhalte in den JSON-Kontext.

Pflichtkontext wird zuerst geprüft und niemals durch Erinnerungen verdrängt.
Zusätzliche Inhalte müssen sowohl die Memory-Grenzen als auch das gesamte
Reasoning-Budget einhalten. Dafür wird die vollständige Wire-Invocation mit
Metadaten und gegebenenfalls Binärdeskriptoren geprüft. Fehlende optionale
Payloadfelder und zu große Erinnerungen werden übersprungen; ein ungültiger
Pflichtkontext bleibt ein Fehler.

## Persistente Auswahl und Parallelität

Die Suche liest einen konsistenten SQLite-Snapshot mit begrenztem Suchumfang.
Der Core speichert die Auswahl anschließend atomar als
`arcs.context_selection.v1`, einschließlich:

- ursprünglichem Auftrag, Policy und Binäreingabe-Auswahl;
- exakten ausgewählten Versionen und Payloadfeldern;
- Suchpfaden, Commit-Grenzen und Suchaufwand;
- Kennzeichnung begrenzter Suche und Anzahl übersprungener Kandidaten.

Das Artefakt referenziert den verwendeten Kontext über Provenienz und
`based_on`. Seine stabile Identität bindet Reasoner, Capability und Request-ID.
Eine andere Policy oder andere Eingaben unter derselben ID werden abgewiesen.
Die Auswahl wird auch gespeichert, wenn keine zusätzliche Erinnerung passt.

Ein späterer Tick oder Neustart verwendet diese Auswahl erneut. Neue Artefakte
oder Beziehungen können sie nicht nachträglich verändern. Ein neuer
Planungsschritt beziehungsweise eine neue Request-ID kann dagegen neu suchen.
So bleibt die Identität laufender paralleler Invocations unverändert.

Auswahl-Artefakte dokumentieren einen Systemvorgang. Sie bestätigen keine
fachliche Wahrheit ihrer Inhalte. Reasoner erhalten die ursprünglichen
Artefakte mit deren Trust und ausschließlich den erlaubten Feldern. Eine
Modellzusammenfassung wird durch Abruf nicht zu einer geprüften Beobachtung.

Die Option gilt für gewöhnliche Reasoning-Routen sowie Planung, initiale
Zielprüfung und Wirkungsbewertung von Goal-Routen. Ein Bewertungs-Reasoner
kann ausgewählte Erinnerungen referenzieren. Frische Vorher-/Nachher-Daten
bleiben für die Wirkungsprüfung weiterhin erforderlich. Erinnerte Ergebnisse
ersetzen weder Verification noch Approval oder aktuelle Gerätevorbedingungen.

## Grenzen und weitere Entwicklung

Projektzuordnung und erlaubte Felder werden zunächst ausdrücklich konfiguriert.
Die automatische Interpretation einer Zielbeschreibung und Auswahl geeigneter
Memory-Scopes gehört zum weiteren Fähigkeits- und Planungsausbau.
`contradicts` und `supersedes` sind definierte Beziehungen; ihr bloßes Vorhandensein
ist noch keine automatische fachliche Konfliktauflösung. Semantische Suche,
Erfahrungskonsolidierung und Qualitätsbenchmarks bleiben offen.

## Prüfen

```bash
cargo test --locked --offline -p arcs-core store::retrieval
cargo test --locked --offline -p arcs-core reasoning::context
cargo test --locked --offline -p arcs-host --test task_runtime
cargo test --locked --offline -p arcs-host --test parallel_runtime
cargo test --locked --offline -p arcs-host --test goal_runtime
```

Die Prozessnachweise prüfen Kontextbildung, Projektgrenzen und Wiederaufnahme
nach verlorenem Aufgabenabschluss. Ein weiterer Test ergänzt Erinnerungen
während eines verzögerten Reasoning-Aufrufs und prüft, dass dessen Kontext
gleich bleibt. Die Goal-Tests verbinden Erinnerungskontext weiterhin mit
frischen Wirkungsdaten und kontrollierten Aktionen.
