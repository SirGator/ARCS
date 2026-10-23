# Allgemeine Datei-Ausgabe

`arcs-file-output` ist ein externer Output-Adapter für JSON-Berichte. Er nutzt
das gemeinsame SDK und enthält weder Core- noch Drohnenwissen. Jede Datei
enthält den vollständigen Ausgabeauftrag einschließlich Report-Payload und
Referenzen auf Inhalt, Auftrag und Freigabe.

## Start und vollständiger Nachweis

Im Workspace-Verzeichnis:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host --example output-report -- \
  apps/adapter-host/config/output.json \
  report-1 apps/adapter-host/config/report.json
```

Das Beispiel übernimmt den Bericht als persistierten Input, erstellt einen
Ausgabeauftrag und prüft ihn mit einer ausdrücklich benannten lokalen
System-Policy: `demo.output.local-file-policy`. Die Policy erlaubt diesen
Bericht für `example.file-output/report.write`. Sie erstellt einen
Verification-Report und ein System-Approval. Erst dann liefert der Host den
Auftrag über stdio aus. Die Ausgabe behauptet keine menschliche Freigabe.

Der Bericht landet unter `var/reports/output-<kodierte-invocation-id>.json`.
Sein Inhalt steht im Feld `payload`; die übrigen Felder belegen die genaue
Zustellung. Der Core-Store liegt in `var/output-host.sqlite`, das Journal
des Adapters in `var/output-journal.sqlite`. Diese Laufzeitdateien werden
von Git ignoriert.

Ein zweiter Lauf mit denselben Parametern liefert dieselben Auftrags-,
Approval- und Belegversionen und erzeugt keine zweite Datei. Veränderte
Berichtsdaten unter derselben Delivery-ID werden abgewiesen. Ein neuer
Bericht benötigt eine neue ID.

```bash
cargo run --locked -p arcs-host -- --config apps/adapter-host/config/output.json
```

Dieser reine Host-Lauf verbindet nur den Adapter. Der allgemeine Host
erzeugt keine Output-Freigaben und veröffentlicht dadurch keinen Bericht.
Die konkrete Beispielpolicy bleibt in `examples/output-report.rs`.

## Grenzen im Core

`OutputService::prepare` bindet eine gespeicherte Inhaltversion, die genaue
Output-Capability, das installierte Manifest und das Ergebnisschema in einem
neuen `arcs.output_request.v1`-Artefakt. Das Vorbereiten liefert noch nichts
aus. Die Freigabe muss genau diesen Auftrag betreffen, einschließlich seiner
Zielbindung. Eine Freigabe des ursprünglichen Inhalts oder eines anderen
Auftrags reicht nicht.

`OutputService::deliver` prüft Request, bestandene Verification,
Approval-Autorität und die zugehörigen Relationen erneut. Der aktuelle Grant
muss die Capability aktivieren und den Adapter als `executor` einstufen.
Capability und Ergebnisschema müssen passen; Output muss Idempotenz und
explizite Berechtigungen deklarieren. Die Dateiquelle benötigt `file.write`.
Ein bloßer Grant ist keine Freigabe für eine bestimmte Zustellung.

Die Core-Bibliothek stellt denselben Ablauf auch ohne stdio bereit. Der
allgemeine Host bietet `prepare_output(...)` und `deliver_output(...)` an.
Die Anwendung entscheidet separat über Verification und Approval.
Grants werden auch beim Wiederverwenden eines fertigen Belegs geprüft.
Receipt, Autoritätsbezüge und erfolgreicher Invocation-State committen im
Core atomar; der World-State wird dadurch nicht automatisch verändert.

## Profil, Dateien und Wiederholung

```json
{"adapter_id":"example.file-output","directory":"../../../var/reports"}
```

Das Zielverzeichnis wird relativ zur Profildatei aufgelöst. Nur der Betreiber
legt es fest; der Report-Payload enthält keinen Ausgabe-Dateipfad. Dateinamen
werden aus der opaken Invocation-ID eindeutig und pfadsicher kodiert.
Der Adapter erwartet zwei Argumente:

```text
arcs-file-output <profile.json> <journal.sqlite>
```

Der Adapter schreibt zunächst eine neue temporäre Datei, synchronisiert
deren Inhalt und veröffentlicht sie durch einen Hardlink ohne Überschreiben.
Das Dateisystem muss diese Operation unterstützen. Unter Unix wird auch das
Verzeichnis synchronisiert. Danach wird die exakte Invocation mitsamt
Empfangsbeleg im SQLite-Journal festgehalten.

Dateisystem und SQLite bilden keine gemeinsame Transaktion. Ein Abbruch
nach der Veröffentlichung und vor dem Journal-Commit ist deshalb ausdrücklich
berücksichtigt: Der nächste identische Auftrag vergleicht den vollständigen
Dateiinhalt und übernimmt den bereits veröffentlichten Bericht. Weil die
Datei den gesamten Auftrag einschließlich Freigabe enthält, reicht gleiche
Report-Nutzlast allein nicht zum Wiedererkennen. Abweichende Inhalte unter
derselben ID oder einem bestehenden Dateinamen führen zu einem Konflikt.
Nach verlorener Antwort bleibt dadurch genau eine veröffentlichte Zieldatei.

Das Journal ist an Adapter-ID und aufgelöstes Zielverzeichnis gebunden.
Ein neues Ziel benötigt eine bewusst getrennte Installation und Identität;
Journal und Ausgabeverzeichnis müssen für die Wiederholungssemantik erhalten
bleiben. Berichte und Journale werden nicht automatisch gelöscht. Bei einem
harten Prozessabbruch können temporäre Dateien zurückbleiben; sie gelten
nicht als zugestellte Berichte.

Der Adapter prüft Schema, Pflichtreferenzen und Idempotenz. Die tatsächliche
Verification und Approval-Gültigkeit prüft der Core; das stdio-SDK ist keine
eigenständige Authentifizierung oder Betriebssystem-Sandbox. Ein Beleg
bestätigt die damalige Zustellung, nicht die dauerhafte Unveränderlichkeit
einer später von außen bearbeiteten oder gelöschten Datei.
