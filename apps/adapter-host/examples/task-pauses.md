# Aufgaben und Pläne pausieren, fortsetzen und abbrechen

Dieser Slice ergänzt `ARCHITECTURE.md`, v3 §8.2 LNG-05 sowie §7.2 PAR-05,
PAR-07 und PAR-10. Eine Aufgabe kann ihre Ausführung unterbrechen, ohne ihre
Identität, Artefakte oder bereits bestätigte Ergebnisse zu verlieren.
Die Steuerung bleibt Bestandteil derselben Task-Runtime und ihres Journals.

## Startbares Beispiel

Die folgenden Befehle laufen vom Workspace-Verzeichnis aus. Das Beispiel nutzt
einen simulierten Ventilator und verlangt manuelle Aktionsfreigaben:

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/task-pauses/host.json \
  --runtime apps/adapter-host/config/task-pauses/runtime.json --ticks 4
```

Aufgabe 1 erreicht `awaiting_approval`. Die vorgeschlagene Aktion wurde noch
nicht ausgeführt. Die Journale liegen getrennt unter `var/task-pauses/`.

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/task-pauses/host.json \
  --runtime apps/adapter-host/config/task-pauses/runtime.json \
  --task-command apps/adapter-host/config/task-pauses/pause.json

./target/debug/arcs-host \
  --config apps/adapter-host/config/task-pauses/host.json \
  --runtime apps/adapter-host/config/task-pauses/runtime.json --ticks 4
```

Der Bericht zeigt `paused_jobs: 1`. Auch nach dem Neustart bleibt derselbe
Kandidat erhalten und es findet keine Geräteaktion statt.

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/task-pauses/host.json \
  --runtime apps/adapter-host/config/task-pauses/runtime.json \
  --task-command apps/adapter-host/config/task-pauses/resume.json

./target/debug/arcs-host \
  --config apps/adapter-host/config/task-pauses/host.json \
  --runtime apps/adapter-host/config/task-pauses/runtime.json --ticks 4
```

ARCS nimmt neue Daten auf und legt einen neuen Kandidaten in Runde 2 zur Prüfung
vor. Eine frühere Freigabe wird nicht in diesen Versuch übernommen. Erst eine
ausdrückliche Freigabe des neuen Kandidaten über den vorhandenen Goal-Review-
Einstieg kann eine Geräteaktion ermöglichen.

Die mitgelieferten Command-IDs gelten für genau diese ersten beiden Änderungen.
Eine identische Wiederholung erzeugt keinen neuen Beleg und wendet die
Änderung nicht erneut an. Für eine weitere Pause braucht der Aufrufer eine neue
Command-ID und die aktuelle Revision.

## Steuerungsvertrag

`--task-command` liest maximal 65.536 Bytes und akzeptiert eine strikt geprüfte
`TaskControlCommand`. Sie kann nicht mit Monitorbetrieb, Ticks oder anderen
Kommandos kombiniert werden. Ein Pausebeispiel:

```json
{
  "sequence": 1,
  "expected_revision": 0,
  "command_id": "project-pause-1",
  "operator": {"actor_type": "human", "id": "local.operator"},
  "reason": "Projekt unterbrechen",
  "change": {"command": "pause"}
}
```

Zum Fortsetzen lautet `change`:

```json
{"command": "resume", "recover_dispatched": false}
```

Dabei sind `expected_revision: 1` und eine neue Command-ID erforderlich.
`TaskRuntime::control_task` bietet denselben Vertrag für eine eingebettete
Anwendung. Die Anwendung authentifiziert den Bediener; ein Actor-String im JSON
ist selbst kein Identitätsnachweis. Nur menschliche oder vertrauenswürdige
Systemakteure werden akzeptiert. Modelle und Adapter erhalten keinen
Steuerungsport und keine zusätzlichen Grants.

Pro Aufgabe beginnt die Kontrollrevision bei 0. Änderungen verlangen die
exakte erwartete Revision und erhöhen sie atomar. Die Command-ID ist mit den
vollständigen ursprünglichen Angaben gebunden. Eine geänderte Wiederholung
wird abgewiesen; eine alte identische Wiederholung kann eine inzwischen erfolgte
Fortsetzung nicht wieder rückgängig machen. Historie und aktueller Zustand
bleiben getrennt. Der Runtime-Einstieg liefert bei Wiederholung den ursprünglichen
Beleg. Die CLI zeigt ihn unter `task_control` neben dem aktuellen Runtime-Bericht.

Die CLI bedient ein gestopptes Journal. Ein laufender Host hält dessen
vorhandene exklusive Sperre. Im laufenden Host nimmt der lokale Monitor den
gleichen Befehl als `kind: "task"` entgegen. Eine eingebettete Anwendung kann
den Runtime-Einstieg zwischen Ticks verwenden. [Monitorsteuerung](task-runtime.md#steuerung-im-laufenden-host)

## Aufgaben abbrechen

Sende einen Aufgabenbefehl mit der aktuellen Revision, einer neuen Command-ID
und `{"command":"cancel"}` als `change`. Die Aufgabe wechselt dauerhaft zu
`cancelled`. Kontrollrevision, Begründung und Beleg mit `cancelled: true` werden
gemeinsam mit dem Aufgabenstatus gespeichert. Wiederhole bei verlorenem Beleg
denselben Befehl, um den ursprünglichen Beleg erneut zu erhalten.

Der Abbruch eines Plan-Hauptauftrags beendet den Plan samt offenen Schritten.
Der Abbruch eines einzelnen Schritts beendet diese Aufgabensequenz. Eine
Abhängigkeit von ihr wird als nicht erfüllt behandelt. Eine konfigurierte
Neuplanung kann ausstehende Arbeit als neue Schritte einplanen. Bei einer
Prüfserie endet der ausgewählte Durchlauf und die Serie behält ihren Zeitplan.

Nach dem Commit verwirft der Host noch nicht übernommene Reasoning- und
Request-Antworten der beendeten Arbeit. Laufende Adapterdialoge halten ihren
Platz bis zur Antwort oder Deadline. Bereits ausgeführte Aktionen behalten
ihre Belege. Eine ungeklärte exklusive Reservierung bleibt bis zur
vorhandenen Betreiberklärung bestehen.

## Wirkung auf Aufgaben, Pläne und Worker

Die Pause liegt über dem bisherigen fachlichen Zustand. Ein Goal kann daher
weiterhin `awaiting_approval` heißen und gleichzeitig durch `paused_by` gesperrt
sein. Der Scheduler wählt solche Aufgaben nicht aus und verbraucht für sie
keinen Ausführungsversuch. Ein pausierter Planer nimmt keinen Vorschlag an;
ein pausierter Root-Plan sperrt auch alle erzeugten Kindaufträge und Joins.

Eltern- und Kindpausen sind unabhängig. Das Fortsetzen eines Kindes hebt eine
Elternpause nicht auf. Das Fortsetzen des Elternplans hebt keine eigene Pause
eines Kindes auf. Diese Regel folgt der vorhandenen einstufigen Planstruktur;
verschachtelte Pläne bleiben weiterhin abgewiesen.

Eine bereits gestartete Reasoning-Berechnung darf antworten. Ein eigener
Abholpfad verarbeitet ausschließlich vorhandene Worker und startet keine neuen
Anfragen. Der Core prüft und speichert Antworten mit denselben Verträgen wie
bei normaler Ausführung. Danach werden Adapter-Session, Workerplatz und
benannte Kapazitäten frei. Die Aufgabe übernimmt das Ergebnis erst nach dem
Fortsetzen. Cache-Replay führt keine zweite Berechnung aus.

Ausstehende Worker behalten ihre Kapazität bis Antwort oder Deadline. Fehler
bleiben Fehler. Wurde ein Fehler bereits im Core gespeichert und ging der
Queue-Checkpoint verloren, verhindert die Pausebindung beim Fortsetzen einen
stillen Neuversuch. Pausieren ist keine neue allgemeine Retry-Policy.

Input und Observe laufen unabhängig weiter. Termin- und Abhängigkeitsbelege
dürfen aktualisiert werden; Abbruch und Gesamtfristen bleiben wirksam.
Auch bei der Abholung pausierter Antworten wird eine noch nicht von der
begrenzten Ablaufvorprüfung erreichte Frist erneut geprüft. Abgelaufene Arbeit
wird über den vorhandenen Abbruchpfad beendet, bevor ihre Antwort übernommen
wird. Eine Pause verlängert `max_lifetime_ms` nicht.

## Goals und bereits freigegebene Aktionen

Vor einem Action-Dispatch beginnt ein fortgesetztes aktives Goal einen neuen
Vorbereitungsversuch: neue Daten, neue Reasoning-/Request-IDs, neue Verification
und eine neue Freigabe gemäß Betreiberpolicy. Der alte Versuch bleibt in der
Core-Historie, zählt aber weiterhin gegen `max_rounds`. Eine Pause darf
Plan-Gesamtbudgets nicht durch zusätzliche unbegrenzte Reasoning-Runden umgehen.
Ist kein Versuch übrig, endet die Fortsetzung mit `budget_exhausted`.

Wurde noch überhaupt kein Goal-Checkpoint erzeugt, kostet das Fortsetzen keinen
zusätzlichen Versuch. Ein gespeicherter Aufnahme-Cursor verhindert, dass alte
Observe-Daten von vor der Fortsetzung die neue Aufnahme erfüllen. Termin- oder
Ereignispausen zwischen vollständig bewerteten Runden behalten ihren bestehenden
Wartepunkt und verbrauchen ebenfalls keinen zusätzlichen Versuch.

Nach einem Dispatch werden seine Wirkung und Bewertung zuerst mit den
ursprünglichen Identitäten fortgeführt. Eine anschließende Planungsrunde beginnt
vor einer neuen Aktion mit frischer Datenerfassung. Bereits laufende
Bewertungen behalten ihre eingefrorenen Belege; das ist keine automatische
Neubewertung eines bereits erreichten Ziels nach beliebiger Zeit.

Ist der Checkpoint noch `executing` und ein Dispatch wurde bereits markiert,
ist dessen Ausgang möglicherweise ungeklärt. Eine normale Fortsetzung wird
abgewiesen. Nach Prüfung des Zustands kann der vertrauenswürdige Aufrufer
`recover_dispatched: true` setzen. Damit erlaubt er ausdrücklich die vorhandene
idempotente Recovery desselben freigegebenen Auftrags. Der Core prüft weiterhin
aktuelle Rechte, Verification und Approval. Der Recovery-Test weist einen
bereits bestätigten Geräteffekt nach einem verlorenen Queue-Checkpoint genau
einmal nach.

Exklusive Goal-Reservierungen bleiben während einer aktiven Pause bestehen.
Ein unbekannter Dispatch wird nicht als beendet oder zurückgerollt dargestellt.
Pausieren ist eine kooperative Scheduler-Sperre, kein physischer Notstopp,
keine harte Unterbrechung synchroner Request-/Act-I/O und keine globale
Aufhebung alter Core-Freigaben. Geräteübergabe und Controller-Fencing bleiben
eigene offene Architekturarbeit. Akzeptierte Jobs und Planreservierungen
behalten ihre Queuekapazität; bestehende Backpressure gilt weiterhin.

## Bericht und Prüfung

Runtime-Berichte und Monitor-Snapshots enthalten:

- `paused_jobs`: gesperrte offene beziehungsweise fehlgeschlagene Aufträge.
- `task_controls`: bis zu 50 aktuelle oder zuletzt angelegte kontrollierte
  Aufgaben, mit eigener Revision, eigener Pause, `paused_by`, Begründung und
  `action_recovery_required`.

`task_controls_after` liefert begrenzte Seiten, `task_control_history` die
unveränderten Kommandos und Belege nach Kontrollrevision. Diese Angaben sind
operative Journalmetadaten und keine neue unabhängige Gedächtnisdatenbank.
Bestehende Runtime-Definitionen und alte Goal-Checkpoints bleiben kompatibel;
die Journalmigration ergänzt nur Tabellen und Spalten.

```bash
cargo test --locked --offline -p arcs-host --test task_suspension
```

Prozess-Tests prüfen Live-Worker/Kapazitäten, Planer-Recovery, Eltern-/
Kindpausen, Optimismus und Idempotenz, Rollback, Fristen, verlorene Fehler- und
Execution-Checkpoints, neue Goal-Freigaben, Versuchslimits, erzeugte Goal-Schritte,
Aufnahme vor dem ersten Checkpoint sowie CLI und Migration. Der Abbruch wird
für einzelne Planschritte, ganze Pläne und eine ungeklärte Reservierung des
simulierten Aktors geprüft.
