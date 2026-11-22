# Allgemeine Aufgabensteuerung bis zur Wirkungsprüfung

Eine Runtime-Route mit `goal` verbindet die vorhandenen Dienste zu einer
persistenten, begrenzten Zielschleife:

```text
Input oder Observe → frische Quellen erfassen → externer Planner
      → Kandidat → Verification → manuelle oder konfigurierte Freigabe
      → Action → Execution → frische Quellen erfassen
      → externer Wirkungs-/Ziel-Reasoner → Outcome
           ├─ erreicht + Wirkung erfolgreich → completed
           ├─ weiterarbeiten → nächster Planungsschritt
           └─ unbekannt / blockiert / Budgetende → anhalten
```

ARCS kennt weder Gerätesemantik noch eine bestimmte KI-Technik. Der Planner
und der Bewertungs-Reasoner sind unabhängig konfigurierte V2-Capabilities;
mehrere Aufgaben und Routen können andere Instanzen, Verfahren und Geräte
verwenden. Die fachlichen Regeln des mitgelieferten Beispiels liegen in
`adapters/example-reasoner/src/goal_fan.rs`, außerhalb von Core und Host.

## Ausführbarer Nachweis

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/goals/host.json \
  --runtime apps/adapter-host/config/goals/runtime.json \
  --ticks 20
```

Bei einer neuen Installation lautet die Aufgabe: den Lüfter mindestens
zweimal umschalten und ausgeschaltet zurücklassen. Ein externer Rules-Planner
schlägt jeweils einen Schaltvorgang vor. Die Betreiberpolicy erlaubt genau
das Gerät `fan-01` und boolesche Schaltwerte. Nach jedem Schritt fragt ARCS den
Zustand separat ab. Ein zweiter Reasoner vergleicht Vorher-/Nachher-Zustand,
Ausführungsbeleg und Aufgabe. Nach zwei Aktionen ist das Ziel `completed`;
die Hardware-Simulation meldet zwei tatsächliche Zustandsänderungen.

Core, Runtime und Adapterjournale liegen unter `var/goals/`. Ein erneuter
Start setzt offene Schritte fort und wiederholt fertige Aufgaben nicht.
Neue Aufgaben kommen mit neuen Ereignis-IDs über den Input-Adapter herein.
Im Desktop-Kontrollpanel ist derselbe Nachweis als **Zielablauf · Lüftertest**
auswählbar. Der Host bleibt nach dem Zielabschluss für neue Aufgaben aktiv.

## Route und Betreiberpolicy

Die bestehenden Route-Felder bestimmen Quelle, Planner, Kandidatenschema,
zulässige Capabilities, Kontextfelder und Ressourcenbudget. `goal` ergänzt:

| Feld | Bedeutung |
| --- | --- |
| `max_rounds` | Höchstens 1–1000 Aktions-/Bewertungsrunden je Aufgabe |
| `initial_assessment` | Optionaler externer Zielcheck vor dem ersten Vorschlag und nach einer Wartepause; Standard false |
| `between_rounds` | Optionales Warten auf einen Termin oder ein frisches Ereignis zwischen abgeschlossenen Runden |
| `max_wait_ticks` | Höchstens 1–10.000 Erfassungsversuche je Quellenphase |
| `sources` | 1–16 explizite Request-/Observe-Quellen mit ausgewählten Payloadfeldern |
| `actions` | Erlaubte Act-Capabilities mit je einem zusätzlichen `verification_schema_id` |
| `approval` | Standard `{"mode":"manual"}`; alternativ ausdrückliche Betreiberpolicy `{"mode":"policy","policy_id":"…"}` |
| `assessment` | Unabhängiger Reasoner, Verfahren, Objective, Constraints und Budget |
| `execution_payload_fields` | Ausgewählte Felder des Ausführungsbelegs für die Bewertung |

Planner und Bewertung liefern jeweils genau einen Kandidaten. Ein
Aktionskandidat verwendet das konfigurierte fachliche Kandidatenschema und
verlangt genau eine der erlaubten Act-Capabilities. Alle Aktionsziele einer
Route müssen dieses Schema akzeptieren und idempotente Ausführung anbieten.
Andere Aufgaben können andere Schemas und Ziele verwenden.

[Wartepausen zwischen Zielrunden](goal-pauses.md) verwenden denselben
Scheduler wie die anfängliche Aktivierung. Nach dem Wecken werden aktuelle
Daten erfasst und neue Handlungsvorschläge erneut geprüft und freigegeben.

Die Verification prüft Planner-Herkunft, Aufgabe, alle ausgewählten
Quellenreferenzen, freigeschaltete Capability und den zusätzlichen
Betreibervertrag. Dieser muss ein geschlossenes Objektschema mit Pflichtfeldern
sein. Beispielsweise begrenzen `enum`, `const`, `minimum` und `maximum` erlaubte
Zielgeräte und Parameter. Der Betreiber muss darin die für seine Anwendung
geeigneten Grenzen festlegen. Ein formal gültiger Payload beweist keine
komplexe physikalische Sicherheit; geräteseitige Invarianten, Revisionen und
Kollisionsprüfungen bleiben Aufgabe der konkreten Adapter.

Nur `policy` erzeugt nach bestandener Verification selbst eine Systemfreigabe.
Ein Reasoner-Vorschlag, auch von einem zweiten Modell, ist keine Freigabe.
Abgelehnte oder fehlgeschlagene Prüfungen erzeugen keine Action. Der Core
prüft die zielgebundene Approval und die aktuellen Grants zusätzlich vor der
Materialisierung und Ausführung. Kandidaten behalten ihre ursprüngliche
Herkunft und ihren Trust; die Runtime hebt Modellvorschläge nicht hoch.

## Quellen und eindeutige Vorher-/Nachher-Zuordnung

Jede Quelle besitzt `id`, `capability`, `schema_id` und `payload_fields`.
Mit `query: {"schema_id":"…","payload":{…}}` verwendet sie Request;
ohne `query` verwendet sie Observe. Optional filtert `subject_id` exakt den
Core-Subject einer Observe-Quelle, damit mehrere Geräte nicht vermischt werden.

Request-Abfragen erhalten pro Aufgabe, Runde, Phase und Quelle eine eigene
persistente Eingabe. Ein Neustart verwendet dieselbe Messung, eine neue
Phase erzeugt eine neue Abfrage. Vor einer Aktion beginnt das Observe-Fenster
am gespeicherten Startcursor. Für die Wirkungsprüfung berücksichtigt die
Runtime Beobachtungen, die nach dem validierten Ausführungsbeleg im Core
eingetroffen sind. Damit kann ein während des Aufrufs empfangener Messwert
die Wirkungsprüfung nicht vorzeitig erfüllen. Dieser Cursor misst den
Empfang im Core. Die physikalische Aufnahmezeit bewertet der externe Reasoner
anhand seiner Quellen.

Für Vorher-/Nachher-Erfassung müssen alle Quellen vorliegen. Teilstände und
Lesepositionen werden gespeichert; historische Logseiten werden begrenzt
gelesen. Fehlende Quellen blockieren nach dem Wartebudget. Eine volle
Aufgabenqueue stoppt neue Inputs, lässt aber die für laufende Ziele nötigen
Observe-Abfragen weiterlaufen.

Reasoning-Kontext wird im Core nach Versionen sortiert. Reasoner dürfen seine
Reihenfolge daher nicht als Zeitfolge interpretieren. Im Goal-Modus enthält
`constraints` ausdrücklich:

```json
{
  "operator_constraints": {},
  "goal_runtime": {
    "phase": "assess",
    "round": 1,
    "task_version_id": "…",
    "before": {"fan-state": "…"},
    "after": {"fan-state": "…"},
    "candidate_version_id": "…",
    "execution_version_id": "…",
    "previous_assessment_version_id": null
  }
}
```

Diese Metadaten ordnen ausschließlich bereits ausgewählte Versionen zu.
Die eigentlichen Daten bleiben in `context`. Beim Weiterplanen erhält der
Planner Aufgabe, letzte Nachher-Daten und die vorige Bewertung. Die Historie
wächst nicht unbegrenzt im Modellkontext.

Eine Quelle kann zusätzlich `binary: {"name":"…","source":{…},
"reference_pointer":"/…"}` konfigurieren. Die Runtime übergibt die aktuellen
Quellversionen an `Host::reason_with_data`. Alle bisherigen Bedingungen gelten:
exakte gespeicherte Referenz, Producer-/Capability-Bindung, ausgewähltes
Referenzfeld, getrennte `data.read`-/`data.receive`-Grants und Mengenlimits.
Kamera und Bildauswertung müssen dafür nicht in ARCS liegen.

### Request-Felder aus gebundenen Vorgängerartefakten

Eine Request-Quelle kann mit `query.from_dependencies` einzelne
Top-Level-Felder des fest konfigurierten Request-Payloads befüllen.
Jeder Eintrag nennt das erwartete Eingabeschema und einen JSON Pointer
in einem bereits gebundenen Abhängigkeitsartefakt.

```json
{
  "revision": {
    "schema_id": "arcs.execution_result.workspace_edit.v1",
    "pointer": "/result/commit"
  }
}
```

Dieser Ausschnitt ersetzt das vorhandene Feld `revision` im
`query.payload`. Der konfigurierte Payload muss bei Start den
Request-Vertrag erfüllen. Die Runtime prüft anschließend den
fertigen Payload bei der Aufnahme ins Artifact-Netz. Der erste
Pointer-Abschnitt muss ein exportiertes Top-Level-Feld ohne
Escape-Sequenz benennen. Genau eine gebundene Artefaktversion muss
Schema und Export erfüllen. Fehlende Werte und mehrdeutige Quellen
blockieren den Schritt vor dem Request-Aufruf.

Die Frage wird an dieselben unveränderlichen Vorgängerartefakte
gebunden, auch wenn eine Quelle vor und nach der Aktion abgefragt wird.
Der Request erhält eine `based_on`-Relation zur verwendeten
Artefaktversion. Abhängigkeitsexporte und Berechtigungen der
Request-Capability gelten weiterhin.

### Nachher-Abfragen an die Ausführungsquittung binden

`query.after_from_execution` übernimmt ausgewählte Werte aus der
persistierten Quittung der Aktion dieser Runde. Das Beispiel bindet
`expected_revision` einer Kameraabfrage an die bestätigte Zustandsrevision.
Vor der Aktion wird weiter der konfigurierte `payload` verwendet.

```json
"query": {
  "schema_id": "arcs.request.drone_camera.v1",
  "payload": {"drone_id": "drone-1", "camera_id": "front"},
  "after_from_execution": {
    "expected_revision": {
      "schema_id": "arcs.execution_result.drone.v1",
      "pointer": "/result/revision"
    }
  }
}
```

Das Zielfeld muss im Abfrageschema deklariert sein. Optionale Felder dürfen
im Vorher-Payload fehlen. Das Wurzelfeld des Pointers muss in
`execution_payload_fields` freigegeben sein. Jede für das Ziel zugelassene
Aktion muss die angegebene Quittungsschema-ID ausgeben. Ein Feld kann
entweder aus einem Abhängigkeitsergebnis oder aus der Quittung gebunden
werden. Die fertige Anfrage wird vor dem Dispatch gegen ihr Schema geprüft.

Die Nachher-Abfrage erhält eine `based_on`-Beziehung zur Quittung.
Beide Versionen bleiben beim Wiederanlauf erhalten. Bei einem fehlenden
Wert oder Schemafehler endet die Runde mit Fehler vor dem Nachher-Request.
Der angefragte Adapter entscheidet anhand seines Vertrags, ob er den
angeforderten Zustand noch bereitstellen kann.

## Externe Wirkungs- und Zielbewertung

Der Bewertungs-Reasoner emittiert `arcs.goal_assessment_candidate.v1`:

```json
{
  "goal_status": "continue",
  "effect": "success",
  "detail": "Die Aktion wirkte; ein weiterer Schritt ist nötig.",
  "evidence_version_ids": ["Version jeder frischen Nachher-Quelle"]
}
```

`goal_status` ist `achieved`, `continue`, `blocked` oder `unknown`.
`effect` ist unabhängig davon `success`, `failure` oder `unknown`.
Alle angegebenen Belege müssen zum übergebenen Kontext gehören, eindeutig
sein und sämtliche aktuellen Nachher-Quellen einschließen. Die Runtime speichert
einen Outcome mit Beziehungen zu Ausführung, externem Bewertungskandidaten
und Belegen. Das dokumentiert die externe Bewertung; es ist kein zusätzlicher
unabhängiger Wahrheitsbeweis des Hosts.

Nur `achieved` zusammen mit `success` beendet die Aufgabe als `completed`.
`continue` mit bekannter Wirkung startet innerhalb des Rundenbudgets eine neue
Planung; auch nach einer fehlgeschlagenen Wirkung kann der Planner reagieren.
Unbekannte Wirkung oder ein blockiertes Ziel hält den Ablauf an.
`no_solution` und `budget_exhausted` eines Reasoners zählen ebenfalls nicht als
Zielerfolg. Ein Planner ohne Aktionsvorschlag blockiert. Mit
`initial_assessment: true` prüft derselbe externe Bewertungsport vorab Aufgabe
und frische Quellen. Er kann ein bereits erfülltes Ziel ohne Aktion bestätigen;
`continue` mit unbekannter Wirkung führt zur ersten Planung. Der vorgezogene
Check erhält `goal_runtime.phase: "check"` und ausschließlich Vorher-Daten.
Er muss alle frischen Quellen belegen. Ohne Ausführung wird kein Outcome
erfunden. Ein Beispiel steht im [Textaufgaben-Nachweis](text-tasks.md).

## Aktionsfelder an die erfasste Quelle binden

Ein Betreiber kann in einem Eintrag von `goal.actions` zusätzlich
`source_matches` setzen. Jeder Eintrag vergleicht den Kandidatenwert
mit dem entsprechenden Wert in der eingefrorenen Vorher-Quelle.

```json
{
  "source_matches": [
    {
      "source_id": "failed-source",
      "candidate_pointer": "/base_commit",
      "source_pointer": "/revision"
    },
    {
      "source_id": "failed-source",
      "candidate_pointer": "/path",
      "source_pointer": "/path"
    }
  ]
}
```

`source_id` bezeichnet eine konfigurierte Goal-Quelle. Beide Pfade
sind JSON Pointer relativ zum jeweiligen Payload. Der erste
Abschnitt muss beim Kandidaten im Verification-Schema deklariert
und bei der Quelle in `payload_fields` enthalten sein. Die
Runtime prüft die Herkunft des gespeicherten Quellenartefakts.
Fehlende oder abweichende Werte lassen die Verification scheitern.
Der Verification-Bericht enthält die betroffenen Felder und
die Quellen-ID. Er speichert für diesen Check keine Feldwerte.

Der Abgleich läuft vor der Freigabe und erneut vor dem Versand
der Action. Die bestehende Prüfung der Kandidatenversion,
ihrer Belege und der Freigaberechte gilt weiterhin.

## Manuelle Entscheidungen und Abbruch

Eine Route ohne ausdrückliche `policy` hält bei `awaiting_approval` an.
Eine vertrauenswürdige Anwendung ruft `TaskRuntime::review_goal` mit
Aufgabennummer, exakter Kandidatenversion, Entscheidung, menschlichem oder
System-Akteur und Begründung auf. Die Anwendung authentifiziert diesen Akteur;
der Port ist kein Adapterkommando.

Für die lokale CLI wird zuerst der laufende Host/das Panel geordnet gestoppt,
damit das Runtimejournal frei ist. Der Bericht beziehungsweise Monitor enthält
unter `goals` die Aufgabennummer und Kandidatenversion. Danach beispielsweise:

```json
{
  "command": "review",
  "sequence": 1,
  "candidate_version_id": "konkrete Version aus dem Bericht",
  "decision": "approved",
  "operator_id": "simon",
  "reason": "Diesen konkreten Schritt geprüft und freigegeben."
}
```

```bash
./target/debug/arcs-host --config <host.json> --runtime <runtime.json> \
  --goal-command <review.json>
```

Dieser lokale Betreiberaufruf speichert die menschliche Entscheidung. Erst
der nächste normale Runtime-Start führt die Aktion aus. `decision: "rejected"`
beendet diese Aufgabe ohne Aktion. Jede neue Runde braucht eine neue Freigabe;
eine verspätete Freigabe für einen alten Kandidaten wird abgewiesen.

`{"command":"cancel","sequence":1,"reason":"Aufgabe zurückgezogen"}`
beendet weitere Verarbeitung. Bereits ausgeführte oder vor einem Absturz
dispatchte Aktionen werden dadurch nicht rückgängig gemacht.

## Erfolg anhand eines Ausführungsbelegs

Eine Goal-Route kann mit `goal.execution_success_schema_id` einen
zusätzlichen Betreibervertrag für das aktuelle Ausführungsartefakt
angeben. Das Assessment muss dessen genaue Version zitieren. Bei
`effect: "success"` prüft der Host den gespeicherten Payload anhand
des Schemas, bevor er das Outcome veröffentlicht.

Eine Route mit `sources: []` benötigt dieses Schema und bewertet erst
nach der Ausführung. Bestehende konfigurierte Quellen bleiben auch mit
einem Erfolgsschema Pflichtbelege. Das [Softwareprüfungs-Beispiel](workspace-check.md#dauerhaftes-prüfziel-starten)
verwendet diese Variante für den Exitstatus eines Testprogramms.

## Persistenz, Betrieb und Grenzen

Das Runtimejournal speichert Zustand, Runde, Quellcursor und Artefaktreferenzen.
Es bleibt an Core, Definition und exklusive Dateisperre gebunden. Alte
Reasoning-/Review-Journale werden unter Erhalt ihrer Jobs migriert. Routen
ohne `goal` behalten ihren bisherigen Vertrag und `ready_for_review`.
Polling-Limits bleiben veränderbar, semantische Konfigurationsänderungen
brauchen weiterhin eine neue Runtime-ID und ein neues Journal.

Vor dem Senden speichert das Journal die Action-Version und den
Dispatch-Cursor. Der Core bindet die Invocation an diese Action und
gegebenenfalls an die gehaltene Reservierungsrevision. Bis zu vier
Act-Worker übertragen solche vorbereiteten Aufrufe. Eine Adaptersitzung
gehört jeweils einem laufenden Aufruf und wird mit den anderen Managern
sowie der Controller-Übergabe geteilt. Im Monitor zeigt `execution_workers`
Aufgabe, Action-Version, Invocation-ID und Capability der laufenden Aufrufe.

Nach einem unterbrochenen Aufruf verwendet die Runtime dieselbe Action- und
Invocation-ID. Der Adapter muss die Wiederholung anhand dieser ID
deduplizieren. Nach einem Abbruch kann eine verspätete Antwort weiterhin
einen Beleg im Artifact-Netz erzeugen. Der Monitor zeigt ihn in der
Reservierungsprüfung. Die Aufgabe bleibt beendet und eine ungeklärte
Gerätereservierung bleibt gehalten.

Aufgaben werden zwischen Schritten fair reihum bearbeitet. Eine manuelle
Freigabepause lässt andere Jobs weiterlaufen. Ein verbuchter Fehler bei einer
Act-Ausführung beendet die betroffene Aufgabe mit `failed`. Andere Aufgaben
laufen weiter, auch bei einem Reasoning- und einem Request-Slot. Der Eintrag
belegt keinen Aufgabenplatz mehr. Eine ungeklärte Gerätereservierung bleibt
bis zur bestätigten Übergabe gehalten.

Beim Wiederanlauf unterscheidet der Core zwischen einem unterbrochenen
Dispatch und einem bereits gespeicherten Act-Fehler. Einen unterbrochenen
Dispatch kann er mit derselben Invocation-ID wiederaufnehmen. Einen
gespeicherten Fehler übernimmt die Runtime in den Aufgabenstatus, auch wenn
dessen Journal-Commit zuvor scheiterte. Eine abgelehnte Veröffentlichung des
Ausführungsbelegs lässt die Invocation für die Wiederaufnahme offen.
Quellenabfragen warten im Request-Pool. Ein verbuchter Quellenfehler
beendet die betroffene Aufgabe und ist im Core nachvollziehbar.
[Quellenabfragen](parallel-requests.md)

`RuntimeReport` ergänzt `completed_goals`, `blocked_goals`, `awaiting_approval`
und eine Liste von höchstens 50 Zielen, aktive zuerst, danach neuere Abschlüsse.
`goals_after(sequence, limit)` ermöglicht
vollständiges seitenweises Lesen. Der Monitor überträgt diese Daten auch an
das Kontrollpanel. Es zeigt Zielzustände im Protokoll und offene Freigaben
im Zähler; Freigabe-Buttons sind noch nicht Bestandteil des Panels.

Die Implementierung bleibt seriell und konfigurationsbasiert: pro Runde eine
Aktion, ein Kandidatenschema je Route, keine automatische Modellwahl und kein
freier Plan-Graph. Sie liefert die allgemeine Koordination für diese Verträge;
autonome Drohnensuche und weitere fachliche Reasoner werden extern angeschlossen.

```bash
cargo test --locked --offline -p arcs-host --test goal_runtime --test task_runtime
```
