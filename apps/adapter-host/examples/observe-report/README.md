# Mehrstufige Adapter-Pipeline mit Wiederaufnahme

Der allgemeine Host bietet mit `arcs_host::pipeline::run_pipeline` eine
wiederverwendbare Ausführung linearer Pipelines. Die Anwendung liefert
Konfiguration und Freigabepolicy. Der Runner nutzt die vorhandenen
Transform- und Output-Dienste.

Das mitgelieferte Beispiel verwendet drei getrennte Prozesse und zwei
Transform-Schritte im selben JSON-Berichtsadapter:

```text
simulated-sensor → gespeicherte Observation und World-State
                → json.report → Bericht mit Evidenzreferenz
                → report.compact → kompakter Bericht, gleicher JSON-Inhalt
                → Ausgabeauftrag → Verification → System-Approval
                → file-output → Datei und persistierter Zustellungsbeleg
```

## Start

Aus dem Cargo-Workspace:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host --example observe-report -- \
  apps/adapter-host/config/pipeline-host.json \
  apps/adapter-host/config/pipeline.json
```

Die Werte **25 °C** und **26 °C** erzeugen je einen Bericht unter
`var/pipeline-v2-reports/`. Core-Datenbank und Adapterjournal liegen unter
`var/pipeline-v2-*.sqlite`. Die V2-Konfiguration verwendet eigene Dateien;
vorherige Beispieldaten bleiben erhalten. Alle Laufzeitdateien sind ignoriert.
Der Aufruf funktioniert aus anderen Verzeichnissen mit absoluten Pfaden zu
beiden Konfigurationsdateien. Pfade im Hostprofil beziehen sich weiterhin
auf dessen Verzeichnis.

Die Ausgabe enthält nur die **in diesem Lauf abgeschlossenen Zustellungen**.
Sie nennt alle Zwischenversionen samt Trust, Auftrags-, Approval- und
Belegversion sowie `completed_cursor` und `remaining_observations`.
Ein weiterer Lauf ohne neue Daten enthält keine Zustellungen und erzeugt
keine weiteren Dateien. Historische Ergebnisse bleiben im Store erhalten.

## Explizite Konfiguration

`pipeline-host.json` lädt Programme, Profile, Schemas und Betreiber-Grants.
Die separate Pipeline-Konfiguration nutzt `config_version: 2`:

```json
{
  "config_version": 2,
  "pipeline_id": "sensor-report-v2",
  "source": {"adapter_id":"demo.sensor","capability_id":"temperature.observe"},
  "input_schema_id": "arcs.measurement.temperature.v1",
  "steps": [
    {"id":"render","capability":{"adapter_id":"example.report","capability_id":"json.report"},"output_schema_id":"arcs.report.v1"},
    {"id":"compact","capability":{"adapter_id":"example.report","capability_id":"report.compact"},"output_schema_id":"arcs.report.v1"}
  ],
  "output": {"adapter_id":"example.file-output","capability_id":"report.write"},
  "receipt_schema_id": "arcs.delivery_receipt.file.v1",
  "max_observations": 100
}
```

Es sind 1 bis 32 Schritte möglich. Pipeline- und Schritt-IDs verwenden
1 bis 64 ASCII-Buchstaben, Ziffern, Punkte, Unterstriche oder Bindestriche;
Schritt-IDs sind innerhalb der Pipeline eindeutig. Derselbe Prozess kann
mehrere Schritte bedienen. Die Tests verwenden auch getrennte Prozesse
für Darstellung und Komprimierung.

Vor dem Polling prüft der Runner die gesamte Kette: aktivierte Observe-
Quelle, Schema-IDs aller Übergänge, Transform-Rollen und eine idempotente
Output-Capability mit Executor-Grant. Es gibt keine automatische Schema-
konvertierung oder Auswahl fachlich passender Geräte. Aktuelle Grants und
Größenlimits werden zusätzlich bei jedem Core-Aufruf geprüft.

### Optionales Transform-Routing

Statt `steps` kann die Pipeline den optionalen Modus `transform_routing`
verwenden. Das Konfigurationsobjekt benennt das Zielschema, eine explizite
Liste von 0 bis 64 Adapter-/Capability-Refs und ein Limit von 1 bis 32
Schritten.

```json
"transform_routing": {
  "output_schema_id": "arcs.report.v1",
  "allowed_capabilities": [
    {"adapter_id":"example.report","capability_id":"json.report"}
  ],
  "max_steps": 1
}
```

Für das bestehende Demo ersetzt dieses Objekt die manuellen `steps` und
findet den kürzesten passenden Transformationspfad von
`arcs.measurement.temperature.v1` nach `arcs.report.v1` über
`example.report/json.report`. Berücksichtigt werden nur registrierte
`accepts`-/`emits`-Verträge und erteilte Grants. Bei gleicher Pfadlänge ist
die Auswahl deterministisch nach Adapter-ID, Capability-ID und Schema
sortiert. Identische Eingabe- und Ausgabeschemas ergeben einen Pfad mit null
Transform-Schritten. Änderungen innerhalb desselben Schemas wie
`report.compact` lassen sich explizit unter `steps` konfigurieren. Die ausgewählte Pfad- und Vertragsidentität wird im bestehenden
Pipeline-Journal gespeichert. Änderungen an der gespeicherten Topologie
erfordern eine neue Pipeline-ID.

Die Ausgabe verwendet die vorhandene Verification und Freigabe-Policy.

Eine Pipeline-ID bindet dauerhaft Quelle, Schritte und deren Reihenfolge,
Ziel, Schema-IDs und die vollständigen Manifeste der beteiligten Adapter.
Änderungen unter derselben ID werden vor dem Polling als Identitätskonflikt
abgewiesen. Auch eine geänderte Adapterversion braucht eine neue Pipeline-ID.
Transform-Implementierungen mit geändertem Ergebnis müssen ihre deklarierte
Version erhöhen. Der JSON-Berichtsadapter nutzt jetzt Version `0.2.0`.
Grants und das Laufbudget gehören nicht zur unveränderlichen Definition.

## Felder am Transform-Eingang auswählen

`input_projection` legt fest, welche Felder die Pipeline aus jeder Observation
an die Transform-Kette übergibt.

```json
"input_projection": {
  "schema_id": "arcs.projection.reading.v1",
  "payload_fields": ["value", "unit"]
}
```

Registriere das Projektionsschema im Hostprofil. Die erste Transform-Capability
muss dieses Schema akzeptieren. Automatisches Routing startet ebenfalls bei
diesem Schema. Erlaubt sind 1 bis 64 eindeutige Felder auf der obersten Ebene.
Der Core übernimmt ausgewählte Werte vollständig und validiert das Ergebnis
gegen das Projektionsschema. Ein fehlendes Feld oder ein unpassender Wert
stoppt die Ausführung vor dem ersten Transform-Aufruf.

Die Projektion bleibt mit der ursprünglichen Observation verknüpft und
behält deren Trust-Obergrenze. Eine Wiederaufnahme verwendet dieselbe
Projektionsversion. In der Zustellung bezeichnet `input_version_id` die
ursprüngliche Observation und `projection_version_id` ihre Projektion.

## Transform im Host abfragen

Für eigene Host-Anwendungen bietet `Host::poll_transform` einen
nichtblockierenden Aufruf mit Capability, Eingabeversion, Zielschema und
Worker-Limit von 1 bis 32. `Ok(None)` bedeutet, dass der Aufruf wartet oder
läuft. Wiederholtes Polling derselben Anfrage liefert das geprüfte Artefakt
als `Ok(Some(artifact))`. Bereits gespeicherte Ergebnisse brauchen keinen
Worker.

Ein laufender Worker belegt seine Adapter-Sitzung und gegebenenfalls
benannte Kapazitäten aus `reasoning_resources`. Der Host schreibt die
Ergebnisse in den Core-Store. Eine Anwendung pollt bis zum Ergebnis oder
Fehler. Beim Shutdown beendet der Host ausstehende Worker. Der
Pipeline-Runner nutzt weiterhin den synchronen Transform-Aufruf.

## Dauerhafter Fortschritt

Der Runner speichert Definition und Checkpoints über den bestehenden
`InputService` als interne Ereignisse des Schemas `arcs.input.host_pipeline.v1`
im selben Core-Store. `arcs.host.pipeline` ist dafür als interne Adapter-ID
reserviert. Es gibt keine zusätzliche Datenbank und keinen direkten Zugriff
auf die privaten Schreibmethoden des Core-Stores.

Ein Checkpoint enthält die gebundene Definitionsversion, den letzten
vollständig abgeschlossenen Observation-Cursor und gegebenenfalls eine
aktive Eingabe mit ihren bisherigen Schritt-, Auftrags-, Approval- und
Belegversionen. Nach jedem erfolgreichen Schritt wird gespeichert.
Fehler an einer Stufe werden mit ihrer Stelle und einer begrenzten Meldung
festgehalten; der Cursor bleibt vor der unvollständigen Eingabe stehen.
Weitere Eingaben werden bei einem Fehler noch nicht verarbeitet.

Bei Wiederaufnahme werden bereits abgeschlossene Schritte durch den
Core-Cache erneut auf Identität, aktuelle Grants und Trust-Grenzen geprüft.
Ihre externen Adapter werden bei einem erfolgreichen Cachetreffer nicht
nochmals aufgerufen. Fehlt ein Checkpoint nach einem bereits abgeschlossenen
Core-Aufruf, liefert dessen dauerhafte Idempotenz dasselbe Ergebnis zurück.
Das gilt auch für einen Zustellungsbeleg vor dem abschließenden Cursor-Commit.
Der ganze Lauf ist keine gemeinsame Transaktion; frühere erfolgreiche
Zustellungen bleiben bei einem späteren Fehler erhalten.

`max_observations` begrenzt 1 bis 1000 **ausstehende Eingaben pro Lauf**.
Alte, bereits abgeschlossene Beobachtungen verbrauchen dieses Budget nicht.
Der Runner pollt einmal, wählt nur die konfigurierte Quelle hinter dem
Cursor und verarbeitet sie in gespeicherter Reihenfolge. Ein Rest wird
über `remaining_observations` sichtbar und beim nächsten Aufruf fortgesetzt.
Die Polling-Limits des Hostprofils gelten zusätzlich.

## Freigabepolicy der Anwendung

`PipelineApproval::approve` ist ein erforderlicher Callback der Anwendung.
Er erhält Quelle, Zwischenstände, Endergebnis und den gespeicherten
Ausgabeauftrag und muss ein bereits persistiertes Freigabeartefakt liefern.
Der Runner erzeugt keine Verification und kein Approval. Eine bereits im
Checkpoint gebundene Freigabe muss bei Wiederaufnahme dieselbe bleiben;
der Callback wird erneut ausgeführt. Der OutputService prüft danach
Entscheidung, Auftrag, Verification und sämtliche Freigaberelationen.

Der Start von **`observe-report` aktiviert ausdrücklich die lokale
System-Policy `demo.pipeline.configured-export.v1`**. Sie erlaubt genau das
abgeleitete Ergebnis am konfigurierten Ziel und erzeugt dafür Verification
und Approval. Eine gespeicherte Ablehnung wird nicht automatisch ersetzt.
Diese Policy bestätigt die Exportentscheidung, nicht die Wahrheit der Daten;
der Trust der Transform-Ergebnisse wird dadurch nicht erhöht.

Die normale Host-CLI startet diese Pipeline nicht und veröffentlicht damit
keinen Bericht. Request- und Transform-Listen der Hostkonfiguration werden
von `observe-report` nicht ausgeführt. Andere Anwendungen können denselben
Runner mit eigener Freigabepolicy verwenden.

## Grenzen und Nachweis

Der Runner arbeitet synchron und begrenzt mit einem aktiven Aufrufer pro
Pipeline und Store. Er ist kein asynchroner Hintergrunddienst oder verteilter
Scheduler. Log und Checkpoint-Historie werden derzeit beim Laden durchlaufen;
der Cursor verhindert erneute Verarbeitung, ersetzt aber noch keinen
gezielten Datenbankindex für sehr große Historien.

Es gibt eine lineare Folge mit einer Eingabe pro Transform und einer
Ausgabesenke. Verzweigungen, Mehrfacheingaben, automatische Binärdatenabrufe,
Bildwahrnehmung und Drohnensuche fehlen noch. Nur Observations verändern den
World-State. Checkpoints, Transform-Ergebnisse und Belege tun das nicht.

Die Dateiausgabe und Recovery bei verlorenen Antworten sind in der
[Adapter-Dokumentation](../../../../adapters/file-output/README.md) beschrieben.
Ein gecachter Beleg bestätigt eine frühere Zustellung; extern gelöschte
Zieldateien werden nicht automatisch neu veröffentlicht.

```bash
cargo test --locked -p arcs-host --test pipeline
cargo test --locked -p arcs-json-report
```

Die Tests prüfen mehrstufige Ausführung über vier Prozesse, Provenienz und
niedrigen Eingangstrust, Fehler erst im zweiten Transform mit anschließender
Wiederaufnahme, neue Beobachtungen hinter dem Cursor, Laufbudgets, gebundene
Definitionen und Manifeste, Quellenauswahl sowie wirksame Freigabegrenzen.
