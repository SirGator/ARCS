# Freigegebene Ausgaben im Dauerbetrieb

## Ergebnis einer Aufgabe ausgeben

Im GTK-Kontrollpanel öffnest du **Aufgaben → Alle Aufgaben**, wählst eine
fertige Aufgabe und klickst auf **Ausgeben …**. Wähle die Ergebnisversion
und den Zieladapter. **Bericht anzeigen** lädt den genauen Bericht zur
Prüfung. Eine Entscheidung braucht eine Begründung. Nach der Zustellung
zeigt der Dialog die Quittung. Dieselbe Vorschau zeigt sie auch nach einem
Host-Neustart.

Der Monitor verwendet dafür den Befehlstyp `task_output`. Bei gestopptem
Host nimmt `--task-output-command command.json` dieselben Befehle an.
Die CLI gibt die Antwort unter `task_output` aus.

Die verfügbaren Ergebnisversionen und Zieladapter liefert diese Anfrage.

```json
{"request_id":"report-options","control":{"kind":"task_output","command":{"command":"options","task_sequence":1}}}
```

Für `preview` enthält `target` dieselbe `task_sequence`, die
`result_version_id` und einen Eintrag aus `destinations` mit
`destination` und `receipt_schema_id`. Die Antwort enthält den Bericht,
die `request_version_id` und die `verification_version_id`.

Für `review` übergibst du ein Objekt `review` mit diesen drei Werten
`target`, `request_version_id` und `verification_version_id` sowie
`operator_id`, `reason` und `decision` mit `approved` oder
`rejected`. Die Runtime protokolliert die Entscheidung und übernimmt die
Freigabe beim nächsten Lauf. `inspect` liest den Stand anhand der
`request_version_id`. Sobald die Antwort eine `delivery_sequence` enthält,
liest der Ausgabeport die Zustellung über diese Sequenz.

Der Bericht enthält die ausgewählte Ergebnisversion und behält deren
Vertrauensniveau. Bei Plänen sind abgeschlossene Schritte der aktuellen
Generation auswählbar. Die Runtime prüft die Zuordnung. Der Core prüft den Zielvertrag.
Die inhaltliche Prüfung übernimmt der Betreiber anhand der Vorschau.

## Berichtsentwürfe aus Arbeitsplänen

Verwende `routes[].plan.templates[].output`, um für eine Schrittvorlage
ein Ziel und das Quittungsschema festzulegen. Der Host prüft den Zielvertrag
beim Laden der Runtime-Konfiguration. Dieses Beispiel verwendet den lokalen
Dateiadapter.

```json
{
  "destination": {
    "adapter_id": "example.file-output",
    "capability_id": "report.write"
  },
  "receipt_schema_id": "arcs.delivery_receipt.file.v1",
  "result_index": 0
}
```

`result_index` ist optional und beginnt bei null. Der Standardwert wählt
das erste Schrittresultat. Der Bericht enthält die Felder aus
`export_fields` der Vorlage. Fehlt das ausgewählte Resultat oder ein
Exportfeld, zeigt der Entwurf einen Fehler an. Die Runtime vermerkt die
Entwürfe zusammen mit dem Abschluss des erfolgreichen Plans. Bei einer
Neuplanung stammen sie aus der erfolgreichen Generation.

Wähle im Kontrollpanel den abgeschlossenen Plan unter **Aufgaben → Alle
Aufgaben** und öffne **Ausgeben …**. Wähle einen Berichtsentwurf und
prüfe seinen Inhalt. **Zustellung freigeben** erfordert weiterhin eine
Begründung. Der Zieladapter ist durch die Vorlage festgelegt.

Im Monitor und in der CLI gibst du bei `options` die Sequenz des Plans an.
Die Liste `plan_outputs` enthält die Sequenz und den Status jedes Entwurfs
(`preparing`, `ready` oder `failed`). Für einen fertigen Entwurf lädt
dieser Monitorbefehl die gespeicherte Vorschau.

```json
{"request_id":"plan-preview","control":{"kind":"task_output","command":{"command":"preview_plan_output","sequence":1}}}
```

Sende für `review` den vollständigen zurückgegebenen `target` zusammen
mit den beiden Nachweisversionen. Die Runtime übernimmt die freigegebene
Ausgabe in dieselbe Zustellwarteschlange wie eine einzeln erstellte Vorschau.

## Freigaben aus anderen Anwendungen übernehmen

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
