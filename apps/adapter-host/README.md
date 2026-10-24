# Allgemeiner ARCS Adapter-Host

`arcs-host` lädt Programme, Profile, Schemas, Grants und optionale
Datenabfragen aus JSON. Das Paket hängt vom Core und Serde/JSON ab, von
keinem konkreten Adapter. Die Beispielprogramme werden als externe Prozesse
gestartet. Fachliche Regeln und die Erteilung von Approvals bleiben in der
jeweiligen Anwendung.

## Start aus dem Workspace-Verzeichnis

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host -- \
  --config apps/adapter-host/config/host.json
```

Alternativ direkt:

```bash
./target/debug/arcs-host --config apps/adapter-host/config/host.json
```

Die mitgelieferte `host.json` verbindet Sensor und Lüfter. Sie übernimmt die
Sensormessungen und fragt beim Lüfter Temperatur und Zustand ab. Die Ausgabe
enthält zuletzt **26 °C** vom Sensor, **31 °C** vom Lüfter-Sensor und einen
**ausgeschalteten Lüfter mit Schaltzähler 0**. Ein Grant für `fan.set` allein
löst keine Aktion aus. Die Beispielsteuerung zum Einschalten bleibt in
`apps/demo-host`.

`config/sensor.json` und `config/fan.json` starten jeweils nur einen Adapter.
Die Beispiele verwenden getrennte Core-Datenbanken unter `var/`; die
Lüfter-Konfigurationen haben auch getrennte Adapter-Datenbanken. `var/` wird
von Git ignoriert. Adapter-Binaries müssen vorher gebaut oder bereitgestellt
werden; der produktive Host baut und installiert keine Programme.

## Konfigurationsformat

```json
{
  "config_version": 1,
  "database": "../../../var/sensor-host.sqlite",
  "node_id": "sensor-host",
  "schemas": [
    "../../../adapters/simulated-sensor/schemas/temperature.schema.json"
  ],
  "adapters": [
    {
      "program": "../../../target/debug/arcs-simulated-sensor",
      "profile": "../../../adapters/simulated-sensor/config/profile.json",
      "args": [],
      "grant_file": "grants/sensor.json",
      "timeout_ms": 5000
    }
  ],
  "polling": {
    "rounds": 1,
    "interval_ms": 0,
    "max_observations_per_adapter": 1000
  }
}
```

Alle relativen **Dateipfade** beziehen sich auf das Verzeichnis der
aufgelösten Konfigurationsdatei, unabhängig vom Aufrufverzeichnis. Das gilt
auch für `program`; es gibt keine automatische PATH-Suche. Absolute Pfade
bleiben absolut. `working_directory` kann pro Adapter gesetzt werden und
ist standardmäßig das Konfigurationsverzeichnis.

Ein optionales `profile` wird als erstes Prozessargument übergeben. Danach
folgen `args`. Strings in `args` sind unveränderte Argumentwerte. Für
dateibezogene Argumente verwendet man ein Objekt mit `path`:

```json
{
  "profile": "../../../adapters/simulated-fan/config/profile.json",
  "args": [{"path": "../../../var/fan-adapter.sqlite"}]
}
```

Für einen Adapter mit benannten Profilargumenten kann `profile` entfallen:

```json
{"args": ["--profile", {"path": "profiles/device.json"}]}
```

Der Host verwendet getrennte Argumente für `Command`, keinen Shell-String.
Die erforderlichen Programme, Profile, Grants, Schema-Dateien und das
Arbeitsverzeichnis müssen existieren. Das Elternverzeichnis der Core-
Datenbank wird angelegt; andere durch Argumente bezeichnete Verzeichnisse
legt der Host nicht selbst an.

## Schemas und Berechtigungen

Die allgemeinen Core-Schemas und der hosteigene Vertrag
`arcs.input.host_request.v1` werden eingebettet geladen. `schemas` ergänzt
die fachlichen JSON-Schema-Dateien. Jede Schema-ID wird einmal geladen;
gemeinsam benutzte Verträge wie Temperatur stehen nur einmal in der Liste.
Alle im Adaptermanifest genannten Verträge müssen vorhanden sein, auch die
von aktuell nicht aktivierten Capabilities.

Der Grant stammt aus einer separaten Betreiberdatei. Er enthält Adapter-ID,
Producer-Klasse, aktivierte Capabilities, Berechtigungen, Trust, Herkunftskanal
und Größenlimits. Das Manifest des gestarteten Prozesses muss dazu passen.
Eine Selbstbeschreibung des Adapters erteilt keine Rechte.

Der Host prüft Dateien und statische Konfiguration zuerst. Danach bindet er
die Schemas im Store und prüft beim Handshake die echten Manifeste und Grants.
Eine Änderung eines bereits gespeicherten Schema-Vertrags unter derselben
ID führt zu `SchemaDrift`; Schema-Evolution braucht eine neue ID.

Adapter-IDs müssen eindeutig sein. `arcs.host`, `arcs.host.pipeline` und
`arcs.host.runtime` sind für interne Input-Dienste des Hosts reserviert. `node_id` ist standardmäßig `adapter-host` und
enthält 1 bis 64 ASCII-Buchstaben, Ziffern, Punkte, Unterstriche oder Bindestriche.
Unabhängige Installationen sollten eigene Node-Namen und zugehörige Adapter-
Datenbanken verwenden. Die ID-Garantie des Stores gilt innerhalb seiner
persistenten Datenbank, nicht zwischen unabhängigen Stores.

Unbekannte Konfigurationsfelder, doppelte IDs, ungültige Schemas, unpassende
Request-Payloads und fehlende Dateien werden sichtbar abgewiesen.

## Externe Inputs und Reasoning

Ein natives [Desktop-Kontrollpanel](../control-panel/README.md) startet diesen
Host mit `--monitor`. Der Modus liefert fortlaufende JSON-Lines für Katalog,
Status, Aufgaben zur Prüfung und Artefakt-Zusammenfassungen. Er benötigt einen
frischen `--stop-file`-Pfad und ist mit einer optionalen Aufgaben-Runtime
kombinierbar. Die bestehenden Grants und Effektgrenzen gelten unverändert.

Alle sieben Rollen können mehrfach angeschlossen sein. `adapter_catalog()`
zeigt die externen Installationen, freigeschaltete Rollen, Schemas und
Reasoning-V2-Verfahren. `capabilities(AdapterRole)` liefert sämtliche passenden
`CapabilityRef`-Ziele, deterministisch sortiert. Die Anwendung wählt einen
konkreten Anschluss. Interne Host-Capabilities erscheinen nicht in dieser
externen Sicht. Mit `--list-adapters` wird derselbe Katalog nach dem Handshake
ohne Polling ausgegeben. Das
[Beispiel mit elf Prozessen](examples/multiple-adapters.md) weist jede Rolle
mehrfach nach, einschließlich getrennt freigegebener Effects und Neustart.

`Host::poll_inputs` übernimmt Aufgaben und Ereignisse von aktivierten
Input-Capabilities. Der Core validiert und persistiert jede Nachricht vor dem
ACK. Dieselbe Nachrichten-ID mit gleichem Inhalt liefert dasselbe Artefakt;
geänderter Inhalt wird abgewiesen. Input aktualisiert weder Current-State noch
World Model automatisch und startet keine Aktion oder Reasoning-Aufgabe.

`Host::reason(ReasoningRequestV2)` ruft einen explizit ausgewählten Reasoner auf.
Die Anwendung bestimmt Kontextfelder, Verfahren, Ziel-Schema, erlaubte
Fähigkeiten und Budget innerhalb des Betreiber-Grants `reasoning_limits_v2`.
Solver, Regeln, LLMs und andere KI-Verfahren verwenden denselben V2-Port.
Modellbasierte Verfahren benötigen die Producer-Klasse `Model`; ihre Ergebnisse
bleiben `Model` mit niedrigem Trust. Alle Ergebnisse sind Kandidaten ohne
automatische Freigabe. Request, Antwort und Kandidaten werden dauerhaft
journalisiert und bei identischen Wiederholungen wiederverwendet.

Das [ausführbare Beispiel](examples/reason-task.md) verbindet eine JSON-Eingabe,
einen mathematischen Solver und einen aufgezeichneten Modell-Testadapter. Es
enthält noch keinen echten Modellanbieter. Die
[Aufgaben-Runtime](examples/task-runtime.md) ergänzt eine dauerhafte synchrone
Polling-Schleife mit konfigurierbarer Zuordnung, persistenter Queue und
Wiederaufnahme. Fertige Kandidaten werden zur fachlichen Prüfung bereitgestellt.

## Konfigurierte Datenabfragen

Gezielte Request-Abfragen liefern einen JSON-Payload. Für größere
Binärinhalte gibt es zusätzlich den unten beschriebenen Referenzabruf.

Ein Adapter kann zusätzlich eine Liste `requests` erhalten:

```json
{
  "requests": [
    {
      "id": "temperature-query-1",
      "capability_id": "temperature.read",
      "input_schema_id": "arcs.request.temperature.v1",
      "response_schema_id": "arcs.measurement.temperature.v1",
      "subject": "temperature-01",
      "payload": {"device_id": "temperature-01"}
    }
  ]
}
```

Der Host persistiert zuerst die vollständige Request-Definition und den
fachlichen Input. Anschließend verwendet er den bestehenden `RequestService`.
Nur der fachliche Input wird an den Adapter weitergegeben. Request-Definition,
Input und Ergebnis sind über den Audit-Trail verbunden.

Eine Request-ID bezeichnet **eine einzelne, dauerhafte Abfrage**, keine
periodisch neue Messung. Ein Neustart mit derselben Definition verwendet das
persistierte Ergebnis. Eine Änderung von Zieladapter, Capability, Schemas,
Subject oder Payload unter derselben ID wird als Identitätskonflikt abgewiesen.
Für eine neue Messung ist eine neue Request-ID nötig. Neue fortlaufende Daten
kommen über Observations; Requests werden einmal vor den Polling-Runden ausgeführt.

Request-IDs sind innerhalb der Datei global eindeutig und verwenden 1 bis
64 ASCII-Buchstaben, Ziffern, Punkte, Unterstriche oder Bindestriche.
Subjects enthalten 1 bis 256 UTF-8-Bytes; der Request-Payload ist auf 60 KiB
begrenzt. Die Capability muss freigegeben sein und tatsächlich den Typ
`Request` mit passenden Eingabe- und Ausgabeschemas anbieten. Eine `Act`-
Capability kann durch einen Eintrag unter `requests` nicht ausgeführt werden.

## Konfigurierte Transforms

`config/transform.json` startet den allgemeinen Zahlen-Normalisierer. Der
Host lädt dessen Schemas und Grant, persistiert Definition und Eingabe und
verwendet anschließend `TransformService`:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host -- --config apps/adapter-host/config/transform.json
```

Ein Adapter kann eine Liste `transforms` erhalten:

```json
{
  "transforms": [{
    "id":"normalize-1",
    "capability_id":"number.normalize",
    "input_schema_id":"arcs.number.raw.v1",
    "output_schema_id":"arcs.number.normalized.v1",
    "subject":"measurement-1",
    "payload":{"value":310.0,"scale":0.1,"offset":0.0,"unit":"Cel"}
  }]
}
```

Die ID bezeichnet eine dauerhafte Definition. Geänderte Parameter oder
Ziele unter derselben ID sind ein Identitätskonflikt; ein neuer Auftrag
benötigt eine neue ID. Request- und Transform-IDs sind innerhalb der Datei
gemeinsam eindeutig und verwenden 1 bis 64 ASCII-Buchstaben, Ziffern,
Punkte, Unterstriche oder Bindestriche. Das Subject benötigt 1 bis 256
steuerzeichenfreie UTF-8-Bytes, der konfigurierte Payload ist auf 60 KiB
begrenzt. Zusätzlich gelten die engeren Grant-Limits.

Nur eine tatsächlich deklarierte Transform-Capability mit passenden
Schema-IDs darf verwendet werden. Requests und Actions lassen sich nicht
als Transform ausführen. Die CLI führt erst konfigurierte Requests, danach
Transforms und anschließend die Polling-Runden aus. Der Bericht enthält
konfigurierte Ergebnisse unter `adapters[].transforms` mit Eingabe- und
Ergebnisversion. Eine Ableitung aktualisiert den World-State nicht automatisch.

`Host::transform(&CapabilityRef, &VersionId, &SchemaId)` verarbeitet dagegen
ein bereits persistiertes Artefakt. Anwendungen können damit zum Beispiel
nach einer Polling-Runde Beobachtungen anderer Adapter verarbeiten. Es gibt
keine automatische Verkettung anhand von Schemanamen und kein implizites
Nachladen referenzierter Binärdaten. `transform_configured(id)` ergänzt
Definition, Input und CLI-Bericht für einen konfigurierten Auftrag.

Die Details zu Trust, Replay und Kontrollgrenzen stehen im
[Transform-Beispiel](../../adapters/normalize-number/README.md).

`Host::request_version(&CapabilityRef, &VersionId, &SchemaId)` führt analog
eine Request-Capability mit einem bereits gespeicherten Query-Artefakt aus.
Die Anwendung erzeugt dessen Eingabe über den InputService. Eine neue
Eingabeversion ermöglicht eine neue Messung; dieselbe Version liefert den
persistierten Snapshot. Auch konfigurierte Requests verwenden diesen Weg.
Der [Drohnen-Bewegungsnachweis](../drone-demo/README.md) nutzt ihn für
getrennte Vorher-/Nachher-Abfragen ohne fachliche Sonderfälle im Host.

## Kontrollierte Ausgabe

`config/output.json` verbindet die allgemeine Dateiausgabe. Der normale
CLI-Lauf veröffentlicht noch keinen Bericht. Eine Anwendung erstellt mit
`prepare_output(&CapabilityRef, &VersionId, &SchemaId)` einen gebundenen
Ausgabeauftrag, prüft diesen und erteilt ein gesondertes Approval.
`deliver_output(&AdapterId, &request_version, &approval_version)` liefert ihn
erst nach erneuter Core-Prüfung aus. Ein Grant allein löst keine Ausgabe aus.

Ein vollständiges Beispiel mit ausdrücklich benannter lokaler System-Policy:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host --example output-report -- \
  apps/adapter-host/config/output.json \
  report-1 apps/adapter-host/config/report.json
```

Die Policy `demo.output.local-file-policy` erlaubt ausschließlich den
konkreten Report für `example.file-output/report.write`. Die Policy liegt
im Beispielprogramm, der allgemeine Host erteilt keine impliziten Freigaben.
Die Ausgabe enthält Auftrags-, Approval- und Belegversion. Der Report liegt
als JSON unter `var/reports/`; identische Wiederholungen erzeugen keine
zusätzliche Zieldatei. Geänderte Berichtsdaten brauchen eine neue Delivery-ID.

Die Freigabe betrifft den Ausgabeauftrag samt Inhaltversion, Ziel-Capability
und Manifest. Ein Approval nur des ursprünglichen Inhalts oder eines anderen
Ziels genügt nicht. Die aktuelle Berechtigung wird auch vor einem Replay
geprüft. Output ist ein eigener Port und keine alternative Act-Ausführung.
Die Anwendung kann dessen Belege anschließend selbst weiterverarbeiten;
der World-State ändert sich durch die Zustellung nicht automatisch.

Profile, Journal und Recovery beschreibt die
[Dateiausgabe-Dokumentation](../../adapters/file-output/README.md).

## Durchgehende Adapterkette

Der allgemeine Runner `arcs_host::pipeline::run_pipeline` verbindet eine
Observe-Quelle, 1 bis 32 konfigurierte Transform-Schritte und eine Output-Senke.
Das Beispiel `observe-report` verwendet ihn mit einer eigenen Exportpolicy.
Die drei Prozesse und ihre Grants stehen in
`config/pipeline-host.json`; Quelle, Schema-Übergänge und Ziele in
`config/pipeline.json`:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host --example observe-report -- \
  apps/adapter-host/config/pipeline-host.json \
  apps/adapter-host/config/pipeline.json
```

Die Messungen 25 °C und 26 °C werden gespeichert, durch den allgemeinen
`json-report`-Adapter dargestellt und nach Verification und System-Approval
unter `var/pipeline-v2-reports/` veröffentlicht. Ein zweiter Transform entfernt
Formatierungsleerzeichen aus dem JSON-Bericht. Die benannte lokale Exportpolicy
liegt im Beispielprogramm. Die normale Host-CLI startet diese Kette nicht.
Ein dauerhafter Cursor überspringt abgeschlossene Eingaben. Unterbrochene
Eingaben verwenden die gespeicherten Zwischenversionen und Zustellungsbelege.

Das Beispiel führt eine Polling-Runde aus und verarbeitet ausstehende
Quellenbeobachtungen innerhalb eines Budgets pro Lauf. Definition und
Checkpoints werden über den InputService im Core-Store gespeichert. Die
Pipeline-ID bindet Topologie, Schemas und Adaptermanifeste; Änderungen
benötigen eine neue ID. Der erforderliche `PipelineApproval`-Callback stammt
von der Anwendung. Dieser Pipeline-Aufruf ist weiterhin begrenzt; die separate
Aufgaben-Runtime verarbeitet Input-/Observe-zu-Reason-Routen. Details zur
Konfiguration (Version 2), Policy, Wiederaufnahme und
den Prozess-Tests stehen in der
[Pipeline-Anleitung](examples/observe-report/README.md).

## Referenzierte Binärdaten

`Host::reason_with_data(request, selections)` kann ausgewählte Referenzen an
einen externen V2-Reasoner weitergeben. Quelle und Empfänger benötigen eigene
Grants. Referenzen müssen aus dem kuratierten Kontext und vom gespeicherten
Producer stammen. Der [Vertrag](../../docs/REASONING.md#explizite-binäreingaben)
beschreibt Limits, Transport und Wiederholungen; der
[externe Pixel-Suchnachweis](../../../drone-sim/adapters/image-reasoner/README.md)
verwendet ihn mit einer Kamera und einem unabhängigen Regel-Reasoner.
Die Aufgaben-Runtime routet weiterhin JSON-Kontext; Binäreingaben werden in
diesem Schritt ausdrücklich über die Host-API ausgewählt.

`config/data.json` startet die allgemeine Dateiquelle. Sie veröffentlicht
kleine Referenzen des Schemas `arcs.data.reference.v1`. Diese werden wie
andere Observations validiert, persistiert und in den World-State übernommen.

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host -- --config apps/adapter-host/config/data.json
cargo run --locked -p arcs-host --example export-data -- \
  apps/adapter-host/config/data.json \
  example.files data.observe sample-v1 /tmp/arcs-sample.txt
```

Das Exportziel muss neu sein, sein Verzeichnis muss existieren. Das Beispiel
veröffentlicht die Datei erst nach dem vollständigen Abruf. Es lässt sich
auch nach einem früheren ACK und einem Neustart ausführen.

Anwendungen verwenden `Host::copy_data(&CapabilityRef, &DataReference,
&mut impl Write)`. Der Host prüft die aktuelle Registry, eine aktivierte
Observe-, Request- oder Transform-Capability, `data.read` sowie `data_limits` im Grant.
Ohne gesetzte Datenlimits ist der Inhalt gesperrt, auch wenn seine Referenz
beobachtet werden darf. Ungültige Limits und die fehlende Permission werden
bereits beim Laden der Konfiguration abgewiesen.

```json
{"data_limits":{"max_object_bytes":16777216,"max_chunk_bytes":8192}}
```

Objektlimit und Blocklimit sind zusätzliche Grenzen; das bisherige JSON-
Payloadlimit bleibt für Metadaten gültig. Maximal sind 64 MiB pro Objekt und
8192 Bytes pro Block möglich. Die Deadline gilt pro Block, es gibt keine
automatische Wiederaufnahme. Der Writer kann bei einem Fehler bereits
vollständige frühere Blöcke enthalten. Der Host speichert standardmäßig nur
die Referenz, nicht die Binärdaten selbst.

Quelle, Persistenz und Einschränkungen beschreibt die
[Dateiquellen-Dokumentation](../../adapters/file-data/README.md).

## Laufzeit und Ausgabe

Ohne `--runtime` führt der CLI-Host einen **begrenzten Lauf** aus. Mit
`--runtime <runtime.json>` arbeitet er dauerhaft bis zum Stop-Signal oder einer
optionalen `--ticks`-Grenze; siehe [Start und Persistenz](examples/task-runtime.md).
Automatische Neuverbindung ist in beiden Modi nicht enthalten. Standardmäßig liest er je Adapter bis
`idle` oder bis zum jeweiligen Budget von 1000 Inputs und Observations und beendet danach alle
Prozesse. Zusätzliche Runden und Pausen sind konfigurierbar:

| Feld | Bedeutung | Standard |
| --- | --- | --- |
| `timeout_ms` je Adapter | Deadline eines stdio-Dialogschritts, 1 bis 3.600.000 ms | 5000 |
| `polling.rounds` | Anzahl der Polling-Runden, mindestens 1 | 1 |
| `polling.interval_ms` | Pause zwischen Runden, 0 bis 60.000 ms | 0 |
| `polling.max_inputs_per_adapter` | Input-Budget je Adapter und Runde, mindestens 1 | 1000 |
| `polling.max_observations_per_adapter` | Budget je Adapter und Runde, mindestens 1 | 1000 |

Input- und Observation-Polling prüfen ihre jeweilige aktivierte Rolle.
Adapter ohne aktivierte Observe-Capability werden nicht nach Observations gepollt. Wenn nur
ein Teil ihrer Observe-Capabilities aktiviert ist, dürfen sie auch nur diese
liefern; nicht freigegebene Daten werden nicht stillschweigend akzeptiert.

Der JSON-Bericht enthält gespeicherte Artefaktzahl, World-Revision, aktuelle
Estimates mit Evidenz und Trust, Request-Ergebnisse sowie Zähler je Adapter.
`poll_limit_hits` zeigt, wie oft ein Budget vollständig verbraucht wurde;
es beweist nicht, dass weitere Daten ausstehen. `inputs` enthält übernommene
Input-Artefakte und `input_limit_hits` die ausgeschöpften Input-Budgets.

Der World-State wird beim Start aus dem gesamten Observation-Log aufgebaut.
Identische Wiederholungen werden bestätigt, ohne den Zustand zurückzusetzen.
Fehler beenden den Lauf mit einem Fehlercode. Bereits erfolgreiche Commits
bleiben erhalten, auch wenn eine spätere Antwort oder ACK-Bestätigung ausfällt.
Der nächste Start baut sie aus dem Log wieder auf. Offene Prozesse werden
auch im Fehlerfall beendet; es gibt keine versteckten Retries oder Fallbacks.

## Einbindung in eine Anwendung

Die Bibliothek `arcs_host` stellt `Host::open`, `request`, `request_version`, `transform`,
`transform_configured`, `reason`, `prepare_output`, `deliver_output`, `copy_data`,
`poll_inputs`, `poll_round`,
`report`, `shutdown` und `execute_approved` bereit. Eine Anwendung kann mit
den Core-Diensten ihre eigenen Vorschläge, Verification und Approvals
erzeugen. `execute_approved` akzeptiert ausschließlich die Version einer
bereits materialisierten, freigegebenen Action und verwendet den bestehenden
`ExecutionService`. Ein Grant, ein Candidate oder eine fehlende Action ist
keine Ausführungsfreigabe.

```rust,no_run
use arcs_host::Host;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = Host::open("apps/adapter-host/config/host.json")?;
    let temperature = host.request("fan-temperature-1")?;
    println!("{}", temperature.payload);
    host.poll_round()?;
    host.shutdown()?;
    Ok(())
}
```

Die alten Demo-Hosts bleiben als Beispiele für fachliche Orchestrierung
erhalten. Der allgemeine Host benötigt keine Änderungen, um einen weiteren
Adapter mit passenden JSON-Verträgen über eine neue Konfiguration anzuschließen.

## Prüfung

```bash
cargo test --locked -p arcs-host --all-targets
cargo test --locked --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features
```

Die Integrationstests starten die echten Simulatoren als separate Prozesse.
Sie prüfen Konfigurationspfade aus einem anderen Arbeitsverzeichnis,
Neustart und Replay, Timeouts, Schema- und Request-Konflikte, Budgets und
die Ausführung über eine separat erteilte Approval-Kette.
