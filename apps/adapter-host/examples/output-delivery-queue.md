# Freigegebene Ausgaben im Dauerbetrieb

Der laufende Host stellt freigegebene Ausgabeaufträge über den Output-Manager
zu. Eine Anwendung erstellt den Auftrag mit `Host::prepare_output` und
speichert eine bestandene Verification sowie eine Approval für genau diese
Auftragsversion. Für die Aufnahme braucht die Runtime die Versionen des
Auftrags und der Approval sowie die ID des vorgesehenen Adapters.

## Auftrag aufnehmen und Zustellung abfragen

Sende an die Standardeingabe eines Hosts mit
`--monitor --control-stdin --runtime …` eine JSON-Zeile dieser Form.

```json
{"request_id":"send-report","control":{"kind":"output","command":{"command":"enqueue","adapter_id":"example.file-output","request_version_id":"REQUEST_VERSION","approval_version_id":"APPROVAL_VERSION"}}}
```

Der Befehl liefert eine dauerhaft gespeicherte Sequenznummer und die
Invocation-ID. Derselbe Auftrag mit derselben Freigabe liefert beim erneuten
Aufruf den bestehenden Eintrag. Mit dieser Sequenz liest der Betreiber den
aktuellen Status.

```json
{"request_id":"read-delivery","control":{"kind":"output","command":{"command":"inspect","sequence":1}}}
```

Bei gestopptem Host unterstützt `--output-command command.json` dieselben
Befehle. Die Datei enthält das Objekt aus `control.command`. Die CLI gibt
den Eintrag unter `output_delivery` aus. Ein anschließender Start mit
`--runtime` bearbeitet die Warteschlange.

## Status und Wiederanlauf

- `queued` umfasst wartende und laufende Zustellungen. Der Monitor nennt
  gerade laufende Invocation-IDs unter `output_workers`.
- `delivered` enthält die Version der geprüften Quittung im Core.
- `failed` enthält den Fehler. Bei einer verlorenen Antwort kann die Wirkung
  bereits eingetreten sein. Ein gespeicherter Fehler bleibt beim Wiederanlauf
  erhalten.

Der Runtime-Bericht zeigt die jüngsten 50 Einträge unter `output_deliveries`.
Jedes Runtime-Journal nimmt höchstens 128 offene Ausgaben auf. Ein Host
verwendet bis zu vier Output-Worker. Eine Adapterinstallation hat eine
gemeinsame Sitzung für alle ihre Rollen. Lokale Hosts mit derselben
Core-Datenbank serialisieren Output-Zustellungen je Adapter per Dateisperre.
Die älteste wartende Ausgabe je
Adapter erhält Gelegenheit zum Versand. Ein belegter Adapter hält Aufträge
anderer Adapter dabei nicht auf.

Der Core autorisiert die exakte Zustellung vor dem Versand erneut und hält
die geplante Invocation fest. Ein Neustart nach einem unterbrochenen Versand
oder einer unterbrochenen Speicherung verwendet dieselbe Invocation-ID.
Adapter müssen diese ID dauerhaft deduplizieren. Ist die geprüfte Quittung
bereits im Core gespeichert, holt die Runtime sie in ihr Journal nach.

```bash
cargo test --workspace --locked --offline --test output_delivery
```
