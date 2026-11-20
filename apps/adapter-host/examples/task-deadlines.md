# Dauerhafte Zeitbudgets für Aufgaben und Pläne

Diese Erweiterung setzt einen Teil von `ARCHITECTURE.md`, v3 §7.2 PAR-07/PAR-10
und §8.2 LNG-01/LNG-06 um. Neben der Deadline eines einzelnen Reasoning-Aufrufs
kann die bestehende Runtime die gesamte Lebensdauer eines Auftrags begrenzen.

## Konfiguration und Beispiel

Eine feste Reasoning-, Plan- oder Goal-Route sowie eine autorisierte
Schrittvorlage kann die zusätzliche Option enthalten:

```json
"max_lifetime_ms": 2592000000
```

Das Beispiel erlaubt 30 Tage ab der Aufnahme in das Runtime-Journal.
Zulässig sind 1 bis 315576000000 Millisekunden, also höchstens zehn Jahre.
Ohne Angabe oder bei `null` entsteht keine eigene Grenze. Die Option bleibt
dann auch in gespeicherten Definitionen unsichtbar; bestehende Routen behalten
ihre Identität. Eine Änderung eines bereits gebundenen Budgets erfordert eine
neue Runtime-ID und ein neues Journal.

Das startbare Beispiel wartet bei einem simulierten Ventilator auf manuelle
Freigabe und beendet die Aufgabe nach fünf Sekunden ohne Geräteaktion:

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/deadlines/host.json \
  --runtime apps/adapter-host/config/deadlines/runtime.json \
  --ticks 80
```

Die Daten liegen getrennt unter `var/deadlines/`. Ohne manuelle Entscheidung
zeigt der Abschlussbericht ein abgelaufenes, abgebrochenes Goal und eine
freigegebene kooperative Reservierung. Das Beispiel nutzt externe
Rules-Reasoner und einen Simulator. Es demonstriert keinen echten Flugcontroller.

## Lebensdauer und Vererbung

Bei der Aufnahme werden Annahmezeit und absolute Ablaufzeit zusammen mit dem
Auftrag gespeichert. Wartephasen für Termine, Ereignisse, Abhängigkeiten und
Freigaben sowie Host-Ausfälle zählen zur Lebensdauer. Ein Neustart oder eine
neue Goal-Runde setzt das Budget nicht zurück. Die Grenze eines erzeugten
Schritts beginnt bei dessen Aufnahme, bevor mögliche Abhängigkeiten erfüllt
sind.

Ein Planbudget gilt auch für alle erzeugten Schritte. Ein zusätzliches
Vorlagenbudget kann die Frist eines Schritts verkürzen. Maßgeblich ist die
frühere Grenze aus Elternplan und eigenem Budget. Ohne eigenes Budget übernimmt
der Schritt die Grenze seines Elternplans. Ein Modell kann diese Werte nicht
in einem Planvorschlag vergeben; sie stammen aus der Betreiberdefinition und
dem eingefrorenen Plan.

Die Runtime speichert den höchsten für Zeitbudgets angenommenen UTC-Zeitpunkt.
Eine später zurückgestellte Uhr verlängert bestehende Budgets nicht und lässt
abgebrochene Aufgaben nicht wieder aufleben. Neue Budgets verwenden ebenfalls
diesen Zeitstand. `tick_at` ermöglicht dieselben Übergänge mit simulierter Zeit.
Der Zeitgeber muss vertrauenswürdig sein: Ein vorwärts springender Zeitstand
kann Aufgaben sofort fällig machen und wird nicht automatisch zurückgenommen.
Nicht darstellbare Ablaufzeiten werden vor der Aufnahme des Auftrags abgewiesen.

## Ablauf und Wiederaufnahme

Zu Beginn eines Ticks prüft die Runtime höchstens `max_jobs_per_tick` fällige
Aufträge. Vor jedem ausgewählten Arbeitsschritt folgt eine weitere Prüfung,
damit eine große Zahl abgelaufener Aufgaben keinen Dispatch durch die
begrenzte Vorprüfung hindurch lässt. Ein noch nicht ausgewählter Auftrag kann
deshalb kurzzeitig weiterhin offen erscheinen, obwohl seine Frist verstrichen
ist. Er darf anschließend keinen neuen Arbeitsschritt starten.

Beim Ablauf wird derselbe bestehende Zustand `cancelled` verwendet. Ein
gesonderter Zeitbudgetbeleg enthält die Ablaufzeit und den tatsächlichen
Abbruchzeitpunkt. Planabbruch, offene Kinder, wartende Joins, Goal-Checkpoints
und Zeitbudgetbelege werden gemeinsam im operativen Journal committed.
Ein kürzeres Schrittbudget bricht den betreffenden Schritt ab; seine
Abhängigkeiten und der Plan werden anschließend als blockiert behandelt.
Ein bereits blockierter Plan mit noch offenen Geschwistern bleibt seiner
Gesamtfrist unterworfen. Fertige Ergebnisse und unveränderliche Core-Artefakte
bleiben erhalten.

Erst nach dem Commit werden die Antworten abgebrochener Reasoning-Aufträge
verworfen. Bereits laufende Worker behalten ihre Adapter- und Kapazitätsplätze
bis zur Antwort oder ihrer eigenen Aufrufdeadline. Eine verspätete Antwort
wird nicht als neues Core-Ergebnis übernommen.

Die bestehende Gerätekoordination gilt auch bei Zeitablauf: Vor einem Dispatch
kann die Reservierung automatisch freigegeben werden. Nach einem ungeklärten
Dispatch bleibt sie erhalten, bis ein passender Abschluss oder eine ausdrückliche
[Klärung](exclusive-goals.md) vorliegt. Das Ablaufdatum ist kein Beleg dafür,
dass ein physisches Gerät frei oder sicher angehalten ist.

## Berichte und Grenzen

CLI-Bericht und `runtime` im Monitor enthalten:

- `expired_jobs`: Zahl der durch Zeitablauf abgebrochenen Aufgaben einschließlich
  betroffener offener Planschritte.
- `next_deadline_unix_ms`: früheste Grenze eines noch offenen Zeitbudgets.
- `task_deadlines`: bis zu 50 Belege, mit offenen Aufträgen zuerst.

Ein Beleg enthält `sequence`, `route_id`, `input_version_id`,
`accepted_at_unix_ms`, `expires_at_unix_ms`, `inherited_from`,
`expired_at_unix_ms` und den aktuellen `task_state`.
`TaskRuntime::deadlines_after(sequence, limit)` liefert zusätzlich Seiten von
bis zu 1000 Belegen. Ein `TickReport` nennt unter `expired_tasks` die direkt in
diesem Tick beendeten Aufträge; kaskadierte Kinder sind im Gesamtbericht sichtbar.

Die Zeitgrenze wird kooperativ zwischen Runtime-Schritten durchgesetzt.
Externe Worker behalten ihre eigenen Dialog-Deadlines. Späte Worker-Antworten
werden nach dem dauerhaft gespeicherten Ablauf des Auftrags verworfen. Im
Live-Betrieb liest der Scheduler die Zeit vor jedem ausgewählten Schritt
erneut. `tick_at` verwendet dagegen für den
gesamten simulierten Tick den ausdrücklich übergebenen Zeitstand. Die Runtime
unterbricht keinen physischen Vorgang in Echtzeit. Bei abgeschaltetem Host
erfolgt der Abbruch beim nächsten Tick. Manuelle Entscheidungen können im Core
erhalten bleiben, erteilen einer abgelaufenen Aufgabe aber keine neue Lebenszeit.

Direkte Act-Aufrufe außerhalb der Runtime sind von dieser Grenze nicht erfasst.
Harte Echtzeitüberwachung, Pause/Resume, Token- und Geldbudgets sowie eine
controllerseitige Besitzprüfung bleiben offen. Ohne konfiguriertes Zeitbudget
gelten die bisherigen Aufruf- und Rundenlimits.

## Prüfung

```bash
cargo test -p arcs-host --test task_deadlines --locked --offline
```

Elf Prozess-Tests prüfen Monatsfristen über Neustarts, Live-Zeit nach langsamer
Beobachtung, verspätete Antworten und
Kapazitätsfreigabe, vererbte und kürzere Schrittgrenzen, blockierte Pläne mit
offener Arbeit, atomaren Rollback, gespeicherte Freigaben, ungeklärte Gerätewirkung,
begrenzte Vorprüfung, Standardkompatibilität und Zeitbereichsfehler.
