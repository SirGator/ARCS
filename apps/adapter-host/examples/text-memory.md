# Textsuche im gemeinsamen Artifact-Gedächtnis

Dieser Slice ergänzt `ARCHITECTURE.md`, v3 §6.5 MEM-02/MEM-08/MEM-09/MEM-10.
Die bestehende Metadaten-/Graphsuche erhält eine optionale lexikalische Suche
über ausdrücklich freigegebene Textfelder. Ältere passende Belege können damit
auch außerhalb des Fensters der jüngsten Metadatentreffer gefunden werden.

## Startbares Beispiel

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/text-memory/host.json \
  --runtime apps/adapter-host/config/text-memory/runtime.json \
  --ticks 12
```

Die Eingabe enthält den internen Suchhinweis `Akku Kalibrierung`. Mehrere
neuere Projektnotizen sind inhaltlich unpassend; eine andere Notiz enthält die
Suchwörter nur im privaten Feld. Eine weitere gehört zu einem anderen Projekt.
Der Solver erhält neben der Zahlenaufgabe ausschließlich das `body` der älteren,
passenden Notiz des erlaubten Projekts. Beide `private_note`-Felder werden durch
die konfigurierte Feldauswahl nicht exportiert. Der Bericht meldet einen
Kandidaten in `ready_for_review`; der Wiederholungsstart erhält denselben
Kontext und ruft den Solver nicht nochmals auf.

Die Journale liegen unter `var/text-memory/`. Der numerische Solver verwendet
die Notiz fachlich nicht. Das Beispiel belegt Textabruf, Projektscope,
Feldauswahl und Wiederaufnahme, keinen gemessenen Vorteil bei der Lösung einer
offenen Sprachaufgabe.

## Konfiguration

Jeder vorhandene Memory-Scope kann optional `text_query` ergänzen:

```json
{
  "filter": {
    "schema_id": "arcs.note.project_memory.v1",
    "subject": "input:12:memory.input:14:memory.capture:15:project-nearest"
  },
  "payload_fields": ["body"],
  "text_query": {"text": "Akku Kalibrierung"}
}
```

Alternativ oder zusätzlich ergänzt `input_fields` ausdrücklich benannte Felder
des **ersten Pflichtkontext-Artefakts** der Reasoning-Anfrage:

```json
{"text_query": {"input_fields": ["private_note"]}}
```

Die Runtime stellt dort die ursprüngliche Aufgabeneingabe bereit. Die Werte
müssen Strings mit mindestens einem Buchstaben oder einer Ziffer sein.
Fehlende, leere, zu lange oder ungültige Suchdaten führen zu einem expliziten
Aufgabenfehler. Diese Felder werden dadurch nicht automatisch exportiert.
Die bisherigen `payload_fields` regeln den tatsächlichen Kontextexport separat.
Eine gültige Anfrage ohne Treffer ergänzt keine Erinnerung; sie fällt nicht
auf die ungefilterte Metadatensuche zurück.

Gesucht werden ausschließlich oberste **Stringfelder aus `payload_fields`**.
Arrays, Zahlen und verschachtelte Objekte werden nicht als Text abgeflacht.
Alle Suchwörter müssen über diese Felder gemeinsam vorkommen. Groß- und
Kleinschreibung wird normalisiert; Satzzeichen trennen Wörter. Es gibt keine
Stammbildung, Synonyme, Embeddings, Phrasen-, Präfix- oder Boolesche Suchsyntax.
`alpha OR beta` sucht auch das Wort `or`. Suchtext wird als gebundener Parameter
und mit einzeln zitierten Wörtern an den Index übergeben.

Schema, Subject und sämtliche Tags gelten weiterhin gemeinsam und bereits vor
der Trefferbegrenzung. Ein Schema allein eröffnet keine globale Textsuche:
Ohne Subject/Tags werden nur Artefakte aus den ausdrücklich erlaubten
Graphpfaden betrachtet. Auch Graphnachbarn müssen den Textfilter erfüllen.
Die Rangfolge bleibt kurze Pfade, danach neuere Core-Commits. Sie verwendet
keine projektübergreifenden Häufigkeitswerte oder ungeprüften Wahrheitsannahmen.

Bei mehreren Scopes wird die Feldauswahl eines tatsächlich passenden Scopes
verwendet. Suchbegriffe in einem nicht freigegebenen Feld können einen anderen
öffentlichen Text nicht zu einem Treffer machen.

## Index und Arbeitsgrenzen

Der Index ist eine löschbare Projektion in derselben SQLite-Datei. Originale,
Versionen, Trust und Beziehungen bleiben im bestehenden Artifact Store.
Pro Schema und sortierter Feldmenge wird eine Projektion geteilt; Subject und
Tags begrenzen die konkrete Suche. Beim Öffnen alter Stores wird kein kompletter
Textscan ausgeführt. Die erste benötigte Suche erzeugt die Tabellen und baut
fehlende Projektionen schrittweise aus der unveränderlichen Historie auf.

| Grenze | Verhalten |
| --- | --- |
| Suchtext | höchstens 4096 Bytes, 1–32 Wörter, höchstens 128 Bytes je Wort |
| Eingabefelder für die Suchanfrage | höchstens 16 unterschiedliche Namen |
| Textfelder | die bestehenden 1–64 freigegebenen `payload_fields` |
| Projektionen | höchstens 128 unterschiedliche Schema-/Feldkombinationen im Store |
| Indexaufbau je Abrufversuch | höchstens 32 indexierte Artefakte insgesamt; zusätzlich ein begrenzter Vorausblick je Projektion |
| Größe je gelesener Artefaktversion | höchstens 262.144 serialisierte Bytes; größere Versionen werden übersprungen |
| Sucharbeit | ungefähr 1.000.000 SQLite-VM-Schritte; Überschreitung liefert `TextSearchWorkLimit` |

Die SQLite-Schrittgrenze begrenzt die Suchausführung, keine garantierte
Wanduhrzeit, Rust-CPU-Zeit oder Betriebssystem-I/O. Der Indexaufbau ist separat
über Stapel- und Artefaktgrößen begrenzt. Bestehende Grenzen für Graphbesuche,
Kanten, Kandidaten, Kontextstücke und gesamte Requestbytes gelten zusätzlich.

Solange eine benötigte Projektion hinter dem aktuellen Snapshot liegt, meldet
der Core `TextIndexPending`. Die Runtime wartet mit
`resource: "memory-text-index"`: `requested` ist die angestrebte Logposition,
`available` die erreichte Position einer noch unvollständigen Projektion.
Es entstehen weder ein teilweiser Kontextbeleg noch ein Reasoning-Worker.
Andere Aufgaben können weiterlaufen; die bestehende Gesamtfrist läuft weiter.

Aufbaufortschritt und Indexeinträge committen atomar. Auch getrennte
SQLite-Verbindungen koordinieren sich über dieselbe kurze Schreibtransaktion.
Wenn alle benötigten Projektionen bereit sind, finden Suchauswahl und
Graphabruf im selben Datenbanksnapshot statt. Der gespeicherte Kontextbeleg
enthält unter `trace.text_indexes` Felder, Snapshot, Indexstand und die kumulativ
in der Projektion übersprungenen übergroßen Versionen. Solche Auslassungen
setzen `trace.truncated`.

## Wiederaufnahme und Wartung

Die Auswahl wird wie bisher als `arcs.context_selection.v1` eingefroren.
Wiederaufnahme prüft Quellen, Suchpolitik, Feldauswahl und Kontextbudgets gegen
die Originalartefakte. Sie benötigt keine neue Suche und keinen vorhandenen
Textindex. Spätere passende Belege ändern erst den nächsten Request.
Aktuelle Grants und die eigentliche Reasoning-Anfrage werden weiterhin geprüft.

Der vertrauenswürdige Store-Einstieg `clear_text_indexes()` löscht ausschließlich
die abgeleiteten Texttabellen. Originale und bereits gespeicherte Kontextbelege
bleiben erhalten. Ein neuer Textabruf baut den Index wieder in begrenzten
Stapeln auf. Erreicht der Store die Projektionsgrenze, liefert eine neue
Feldkombination ausdrücklich `TextIndexCapacity`; es gibt keinen Vollscan als
stillschweigenden Ersatz.

Bestehende Memory-JSON-Konfigurationen behalten ihre bisherige Identität und
brauchen keine Änderung. Rust-Aufrufer verwenden nun
`RetrievalQuery.scopes: Vec<RetrievalScope>`; ein Scope kombiniert den bisherigen
`ArtifactFilter` mit optionalem `PayloadTextQuery`. `MemoryScope` ergänzt das
optionale Feld `text_query`.

## Prüfung

```bash
cargo test --locked --offline -p arcs-core store::retrieval
cargo test --locked --offline -p arcs-core reasoning::context
cargo test --locked --offline -p arcs-host --test text_memory
```

Ein deterministischer Vergleich verwendet eine ältere relevante Notiz, 100
neuere unpassende Notizen und einen gleichlautenden fremden Projektbeleg.
Beide Verfahren dürfen höchstens drei Versionen besuchen:

| Verfahren | Gefundene relevante Notiz | Fremde Projektbelege |
| --- | --- | --- |
| Bisherige Metadaten-/Graphsuche | 0 von 1 | 0 |
| Textsuche mit denselben Scope- und Besuchsgrenzen | 1 von 1 | 0 |

Das ist ein gezielter Regressionstest für dieses Abrufproblem. Er ersetzt
keinen vielfältigen Qualitätsbenchmark und misst weder LLM-Antwortqualität
noch Tokenersparnis. Weitere Tests prüfen SQL-Arbeitsabbruch und anschließende
Store-Nutzbarkeit, Unicode-Normalisierung, übergroße Versionen, konkurrierenden
Indexaufbau, Löschung/Neubau, überlappende Scopes, private Daten, fehlende
Suchfelder und den verlorenen Abschluss-Checkpoint.
