# Warten zwischen abgeschlossenen Zielrunden

Dieser Schritt erweitert die bestehende Goal-Runtime entlang
[ARCHITECTURE.md](../../../docs/ARCHITECTURE.md), §8.2 LNG-03 bis LNG-07.
Eine Aufgabe kann nach einer vollständig ausgeführten und bewerteten Runde
auf einen Termin oder ein neues Ereignis warten. Die Aufgabe, ihre bisherigen
Wirkungsbelege und ihr Memory-Scope bleiben erhalten.

## Startbares Beispiel

Das Beispiel steuert ausschließlich den bestehenden Lüftersimulator. Die
Aufgabe verlangt zwei Schaltvorgänge und den abschließenden Zustand „aus“.
Zwischen den beiden Runden wartet sie 30 Sekunden:

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/goal-pauses/host.json \
  --runtime apps/adapter-host/config/goal-pauses/runtime.json \
  --ticks 20
```

Beim ersten Start zeigt der Bericht `waiting_time: 1`, einen Goal-Zustand
`waiting_time` und einen bereits bestätigten ersten Schritt. Der Host kann
beendet werden. Nach Ablauf des gespeicherten Termins denselben Aufruf erneut
ausführen: Die Aufgabe erfasst neue Zustandsdaten, plant den zweiten Schritt
und bestätigt den Abschluss. Ein weiterer Start führt keine zusätzliche
Aktion aus. Journale liegen getrennt unter `var/goal-pauses/`.

## Betreiberkonfiguration

`goal.between_rounds` verwendet denselben Vertrag wie die optionale
[anfängliche Aktivierung](scheduled-runtime.md):

```json
"goal": {
  "initial_assessment": true,
  "between_rounds": {"kind": "after_delay", "delay_ms": 2592000000},
  "max_rounds": 4
}
```

Dieser Ausschnitt wird in eine vollständige Goal-Konfiguration eingefügt.
Er setzt zwischen fortzuführenden Runden eine Pause von 30 Tagen.
`after_delay` beginnt mit dem erfolgreichen Commit der Wartephase und erhält
seine Fälligkeit über Neustarts. `at_time` wartet auf einen absoluten
UTC-Unixzeitpunkt in Millisekunden; für spätere Runden liegt derselbe Termin
möglicherweise bereits in der Vergangenheit.

Für ein externes Signal:

```json
"between_rounds": {
  "kind": "on_event",
  "source": {"adapter_id": "project.events", "capability_id": "status.changed"},
  "schema_id": "project.status.v1"
}
```

Quelle und Schema müssen als Input- oder Observe-Capability registriert und
aktuell freigegeben sein. Ein optionaler `subject` filtert die vollständige
Core-Subject-ID. Pro Runde gilt eine neue Ereignisgrenze: Das Signal muss
**nach dem Beginn dieser Wartephase** im Core erscheinen. Ein altes Signal
weckt keine spätere Runde erneut. Der begrenzte Ereignisabruf und die
Weiterleitung benötigter Signale bei voller Queue gelten auch hier.

Für eine regelmäßige Überprüfung, die bei einer Zustandsmeldung früher
starten darf, nutze `on_event_or_after_delay` aus der
[Scheduler-Konfiguration](scheduled-runtime.md#ereignis-mit-spätestem-termin).
Jede Wartephase speichert einen eigenen Termin und eine eigene Ereignisgrenze.
Beide Wege starten die neue Runde mit frischen Zustandsdaten.

Ohne `between_rounds` bleibt das bisherige Verhalten erhalten. Eine neue
oder geänderte Wartebedingung verändert die fachliche Route und braucht eine
neue Runtime-ID und ein neues Journal.

## Ablauf und Sicherheitsgrenzen

Eine Pause entsteht erst, wenn der Bewertungs-Reasoner frische Wirkungsdaten
ausgewertet hat, `continue` mit `success` oder `failure` meldet und noch eine
weitere Runde erlaubt ist. Nicht abgeschlossene Aktionen oder laufende
Modellaufrufe werden dafür nicht unterbrochen. Unbekannte Wirkung, Ablehnung
und ausgeschöpftes Rundenbudget bleiben wie bisher sichtbar angehalten.

Die Runtime speichert nächste Rundennummer, Goal-Checkpoint, Wartebedingung
und Ereignisgrenze in einer Transaktion. Jede weitere Wartephase erhält eine
neue Aktivierungsrevision. Frühe Aktivierungen und ihre Weckbelege werden
nicht überschrieben. Ein Speicherfehler nach einer bereits gespeicherten
Wirkungsbewertung wird beim expliziten Wiederaufnehmen mit denselben Core-
und Adapter-Aufrufen aufgearbeitet; es entsteht keine zweite Gerätewirkung.

Nach dem Wecken wechselt das Ziel auf `acquiring`. Vor der neuen Erfassung
wird eine aktuelle Core-Loggrenze gespeichert. Request-Quellen erhalten eine
neue Abfrage für diese Runde; Observe-Quellen benötigen Beobachtungen nach
dieser Grenze. Bereits während der Pause gespeicherte Beobachtungen gelten
damit nicht automatisch als aktueller Zustand. Fehlen neue Beobachtungen,
greift weiterhin `max_wait_ticks`.

Alte Kandidaten, Verifications, Approvals und Ausführungen bleiben in der
Core-Historie. Sie werden aus dem aktiven Handlungskontext entfernt. Jeder
neue Handlungsvorschlag benötigt erneut Verification und eine passende
manuelle oder ausdrücklich konfigurierte Systemfreigabe. Eine verspätete
Freigabe für den Kandidaten der vorigen Runde wird abgewiesen.

Mit `initial_assessment: true` bewertet ein dafür geeigneter Reasoner nach
dem Wecken zunächst den neuen Zustand. Ist das Ziel inzwischen anderweitig
erreicht, kann es ohne weitere Aktion abgeschlossen werden. Der mitgelieferte
`fan_goal`-Rules-Reasoner unterstützt diesen Check; nach einer eigenen Aktion
verlangt seine Wirkungsbewertung weiterhin separate Vorher-/Nachher-Daten.
Memory-Abruf und Kontextbelege werden für den neuen Reasoning-Auftrag nach
den vorhandenen Scope- und Budgetregeln gebildet.

Wartezeit verbraucht keine zusätzlichen Reasoning-Worker und zählt nicht
gegen `max_wait_ticks` der Datenerfassung. `max_rounds` begrenzt weiterhin
alle Aktionsrunden, auch bei Wartezeiten von null. Ein wartendes Goal lässt
sich über den vorhandenen vertrauenswürdigen `cancel_goal`-Port beziehungsweise
`--goal-command` abbrechen. Danach wecken weitere Termine oder Signale es
nicht mehr. Der Abbruch nimmt frühere Wirkungen nicht zurück.

## Status, Migration und Nachweis

Goal-Berichte und das Kontrollpanel kennen `waiting_time` und `waiting_event`.
Der Gesamtbericht zählt wartende Aufgaben weiterhin zu `pending_jobs` und
zeigt den nächsten fälligen Zeitpunkt. `scheduled_after(sequence, limit)`
liefert die letzte Aktivierungsrevision je Aufgabe. Mit
`scheduled_history(sequence, after_revision, limit)` lässt sich die gesamte
Aktivierungshistorie einer Aufgabe lesen: `None` beginnt bei Revision 0,
danach wird der zuletzt gelesene Revisionswert übergeben. Die Seitengröße
beträgt 1–1000. `ready` beschreibt eine erfüllte Aktivierungsbedingung und
ersetzt keinen Abschlussbeleg des Goals.

Vorhandene Scheduler-Journale werden transaktional migriert. Ihre bisherigen
Aktivierungsbelege erhalten Revision 0; IDs, Fälligkeiten, Lesepositionen und
Weckversionen bleiben erhalten. Unveränderte Routen ohne `between_rounds`
behalten ihre serialisierte Identität und ihr Ausführungsverhalten.

```bash
cargo test --locked --offline -p arcs-host \
  --test goal_waits --test scheduled_runtime --test goal_runtime
```

Die echten Prozess-Tests prüfen wiederholte Monatspausen mit parallelem
Reasoning und Memory, Neustarts vor und nach dem Wecken, neue Daten und
manuelle Freigaben, externe Zustandsänderungen, frische Signale bei voller
Queue, Abbruch, Rundenbudget, alte Journale und einen gezielt fehlgeschlagenen
Warte-Commit nach einer bereits ausgeführten Aktion.

Die Wartebedingung wird derzeit durch den Betreiber für die Route bestimmt.
Dynamisch durch Reasoner vorgeschlagene Wartepläne, beliebiges Pausieren
innerhalb einer laufenden Runde, Kalender-/Cron-Termine und allgemeine
Teilzielabhängigkeiten bleiben offen.
