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

Adapter-IDs müssen eindeutig sein. `arcs.host` ist für den internen Input-
Dienst des Hosts reserviert. `node_id` ist standardmäßig `adapter-host` und
enthält 1 bis 64 ASCII-Buchstaben, Ziffern, Punkte, Unterstriche oder Bindestriche.
Unabhängige Installationen sollten eigene Node-Namen und zugehörige Adapter-
Datenbanken verwenden. Die ID-Garantie des Stores gilt innerhalb seiner
persistenten Datenbank, nicht zwischen unabhängigen Stores.

Unbekannte Konfigurationsfelder, doppelte IDs, ungültige Schemas, unpassende
Request-Payloads und fehlende Dateien werden sichtbar abgewiesen.

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

## Referenzierte Binärdaten

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
Observe- oder Request-Capability, `data.read` sowie `data_limits` im Grant.
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

Dieser CLI-Host führt einen **begrenzten Lauf** aus. Er ist kein automatisch
neu verbindender Hintergrunddienst. Standardmäßig liest er je Adapter bis
`idle` oder bis zum Budget von 1000 Observations und beendet danach alle
Prozesse. Zusätzliche Runden und Pausen sind konfigurierbar:

| Feld | Bedeutung | Standard |
| --- | --- | --- |
| `timeout_ms` je Adapter | Deadline eines stdio-Dialogschritts, 1 bis 3.600.000 ms | 5000 |
| `polling.rounds` | Anzahl der Polling-Runden, mindestens 1 | 1 |
| `polling.interval_ms` | Pause zwischen Runden, 0 bis 60.000 ms | 0 |
| `polling.max_observations_per_adapter` | Budget je Adapter und Runde, mindestens 1 | 1000 |

Adapter ohne aktivierte Observe-Capability werden nicht gepollt. Wenn nur
ein Teil ihrer Observe-Capabilities aktiviert ist, dürfen sie auch nur diese
liefern; nicht freigegebene Daten werden nicht stillschweigend akzeptiert.

Der JSON-Bericht enthält gespeicherte Artefaktzahl, World-Revision, aktuelle
Estimates mit Evidenz und Trust, Request-Ergebnisse sowie Zähler je Adapter.
`poll_limit_hits` zeigt, wie oft ein Budget vollständig verbraucht wurde;
es beweist nicht, dass weitere Daten ausstehen.

Der World-State wird beim Start aus dem gesamten Observation-Log aufgebaut.
Identische Wiederholungen werden bestätigt, ohne den Zustand zurückzusetzen.
Fehler beenden den Lauf mit einem Fehlercode. Bereits erfolgreiche Commits
bleiben erhalten, auch wenn eine spätere Antwort oder ACK-Bestätigung ausfällt.
Der nächste Start baut sie aus dem Log wieder auf. Offene Prozesse werden
auch im Fehlerfall beendet; es gibt keine versteckten Retries oder Fallbacks.

## Einbindung in eine Anwendung

Die Bibliothek `arcs_host` stellt `Host::open`, `request`, `poll_round`,
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
