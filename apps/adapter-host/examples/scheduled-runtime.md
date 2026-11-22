# Persistente Termin- und Ereignisaktivierung

Dieser Schritt setzt Teile von [ARCHITECTURE.md](../../../docs/ARCHITECTURE.md)
§7.2 PAR-06 sowie §8.2 LNG-01, LNG-03 und LNG-04 um. Eine angenommene Aufgabe
kann vor ihrem ersten Schritt auf einen Termin oder ein externes Ereignis
warten. Ihre ursprüngliche Core-Eingabe und Route bleiben erhalten.
Wartende Aufgaben belegen keinen Reasoning-Worker und rufen kein Modell auf.

## Startbares Beispiel

Das Beispiel verwendet den bestehenden mathematischen Solver. Eine Eingabe
erzeugt drei Aufgaben: eine sofortige Berechnung, dieselbe Berechnung nach
30 Sekunden und eine Berechnung nach einem passenden externen Signal.

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/scheduled/host.json \
  --runtime apps/adapter-host/config/scheduled/runtime.json \
  --ticks 10
```

Beim ersten Lauf zeigt der Bericht ein Ergebnis zur Prüfung, eine Aufgabe
unter `waiting_time` und eine unter `waiting_event`. Die Journale liegen
getrennt unter `var/scheduled/`. Der Host darf zwischen den Läufen beendet
werden. Nach Ablauf des Termins bearbeitet der nächste Lauf die zeitlich
wartende Aufgabe. Das Ereignis lässt sich separat einreihen:

```bash
./target/debug/arcs-json-input --enqueue \
  apps/adapter-host/config/scheduled/wake-profile.json \
  var/scheduled/wake.sqlite \
  apps/adapter-host/config/scheduled/wake-event.json
```

Danach denselben Host-Aufruf erneut ausführen. Sind Termin und Signal
eingetreten, enthält der Bericht insgesamt drei Ergebnisse zur Prüfung.
Weitere Starts wiederholen fertige Berechnungen nicht. Im Dauerbetrieb
entfällt `--ticks`; die vorhandene `--stop-file`-Option beendet den Host.

## Aktivierungsbedingungen

Eine Route kann genau eine optionale `activation` angeben. Ohne diese Option
startet sie wie bisher sofort. Absolute Termine verwenden UTC-Unixzeit in
Millisekunden:

```json
"activation": {"kind": "at_time", "unix_ms": 1893456000000}
```

Eine relative Wartezeit beginnt mit der **dauerhaften Aufnahme in die
Runtimequeue**, nicht mit dem Start des Hosts oder der Erzeugung der externen
Nachricht. Die errechnete Fälligkeit wird einmal gespeichert und durch einen
Neustart nicht verschoben:

```json
"activation": {"kind": "after_delay", "delay_ms": 2592000000}
```

Das sind 30 Tage. Relative Wartezeiten sind auf zehn Jahre begrenzt; absolute
Zeitpunkte müssen in den nichtnegativen SQLite-`i64`-Bereich passen. Überfällige
Aufgaben werden beim nächsten Tick bereit. Einmal aktivierte Aufgaben bleiben
auch nach einem Rücksprung der Systemzeit aktiv. Noch wartende Termine folgen
der UTC-Systemzeit. Eine schlafende oder ausgeschaltete Maschine wird durch
den Scheduler nicht gestartet.

Ein Ereignis wird anhand seiner **Core-Herkunft**, Capability, Schema-ID und
optional seiner vollständigen Core-Subject-ID ausgewählt:

```json
"activation": {
  "kind": "on_event",
  "source": {"adapter_id": "schedule.events", "capability_id": "task.submit"},
  "schema_id": "arcs.task.nearest.v1",
  "subject": "input:15:schedule.events:11:task.submit:15:project-nearest"
}
```

`subject` bezeichnet die namespacierte ID des gespeicherten Artefakts, nicht
den unqualifizierten externen Themenbezeichner. Sie ist optional; ohne sie
gilt jedes spätere Ereignis derselben Quelle und desselben Schemas. Quelle
und Schema müssen durch eine aktuelle Input- oder Observe-Capability
autorisiert sein. Der Host prüft diese Autorisierung vor jeder Runde erneut.
Subject-Filter sind auf 1–256 Bytes begrenzt.

Es zählen ausschließlich Core-Logeinträge **nach der ursprünglichen
Aufgabeneingabe**. Frühere Signale wecken eine neuere Aufgabe nicht. Ein
Signal nach der Eingabe zählt auch dann, wenn die Aufgabe erst später wegen
Rückstau aufgenommen wird. Ein Signal darf mehrere passende Aufgaben wecken.
Die Signalversion bleibt im Aktivierungsbeleg gespeichert. Sie erteilt keine
Freigabe und wird nicht automatisch als zusätzlicher Reasoning-Kontext oder
Nachweis einer erfolgreichen Geräteaktion übernommen.

### Ereignis mit spätestem Termin

Mit `on_event_or_after_delay` kann ein Signal die Aufgabe früh wecken.
Falls der Scheduler bis dahin kein passendes Signal verarbeitet hat,
wird die Aufgabe zum gespeicherten Termin bereit.

```json
"activation": {
  "kind": "on_event_or_after_delay",
  "source": {"adapter_id": "schedule.events", "capability_id": "task.submit"},
  "schema_id": "arcs.task.nearest.v1",
  "delay_ms": 2592000000
}
```

Beide Wege schreiben denselben Aktivierungsbeleg. Pro Tick gibt der
Scheduler zuerst fällige Termine frei. Ein noch ungelesenes Signal
kann einen bereits fälligen Termin deshalb auch nach einem längeren
Ausfall nicht ersetzen. Bei der Freigabe durch ein Ereignis enthält
der Beleg dessen Version. Bei der Freigabe durch den Termin bleibt
das Feld leer.

Der Wartezustand zählt zu `waiting_event`. `fallback_at_unix_ms` nennt
im Aktivierungsbeleg und in der Queue die gespeicherte Fälligkeit.
Sie fließt auch in `next_wake_unix_ms` ein.

## Persistenz, Last und Wiederaufnahme

Queue-Aufnahme, Aktivierungsbedingung, Fälligkeit und Discovery-Cursor werden
in derselben SQLite-Transaktion gespeichert. Ereigniswartebedingungen haben
jeweils einen eigenen dauerhaften Core-Cursor. Der Scheduler liest pro Runde
höchstens `max_jobs_per_tick` solche Wartebedingungen und je Bedingung höchstens
`max_events_per_tick` Logeinträge. Eine gespeicherte Umlaufposition verteilt
dieses Budget auch über Neustarts auf die wartenden Aufgaben. Cursor und
Weckbeleg werden gemeinsam committed. Nach einem ACK, aber vor dem Wecken,
bleibt das Ereignis im Core auffindbar.

Wartende Aufgaben zählen weiterhin zu `max_pending_jobs`. Bei voller Queue
fragt der Host ausschließlich die Input-Adapter noch offener
Ereigniswartebedingungen weiter ab; Beobachtungen bleiben ebenfalls
abrufbar. Die normalen Ingress-Limits gelten weiter. Diese Ausnahme verhindert
einen Stillstand durch Rückstau. Sie arbeitet pro Adapterinstallation:
Liefert derselbe Adapter außerdem neue Aufgaben, können diese bereits im
Core-Log auf freie Queueplätze warten. Das Core-Log hat dadurch keine neue
Speicherobergrenze; Speicherquoten und allgemeine Aufbewahrungsregeln bleiben
eine eigene Betriebsaufgabe.

Eine freigegebene Aufgabe nutzt anschließend unverändert die bestehenden
Aufruf-IDs, Journale, Memory-Scopes und Sicherheitsprüfungen. Ein Termin oder
Signal ersetzt keine Verification oder Approval. Additive Scheduler-Tabellen
erhalten vorhandene Journale, IDs und Fertigmeldungen. Die optionale Einstellung
fehlt bei unveränderten Routen auch in deren serialisierter Identität.
Eine neue oder geänderte Aktivierungsbedingung ändert dagegen die fachliche
Route und benötigt wie andere Routenänderungen eine neue Runtime-ID und ein
neues Journal.

## Status und Grenzen

Berichte enthalten `waiting_time`, `waiting_event` und `next_wake_unix_ms`.
Das Kontrollpanel zeigt die Anzahl zeit- oder ereigniswartender Aufgaben.
Die Bibliotheksmethode `TaskRuntime::scheduled_after(sequence, limit)` liefert
bis zu 1000 Aktivierungsbelege je Seite samt Eingabeversion, Fälligkeit,
Leseposition und gegebenenfalls Signalversion. Ihr Zustand `ready` beschreibt
die erfüllte Aktivierungsbedingung; den Bearbeitungsstand zeigt weiterhin die
Aufgabenqueue beziehungsweise Goal-Runtime.

`tick_at(host, unix_ms)` ermöglicht einer vertrauenswürdigen Host-Anwendung
simulierte Schedulerzeit. `tick(host)` verwendet die Systemzeit. Die Methode
verändert weder die Uhren von Adaptern noch die Zeitquelle bestehender
Verification-/Approval-Dienste.

Die Route-Einstellung `activation` ist eine **einmalige Aktivierung vor dem
ersten Aufgabenschritt**. Goal-Routen können zusätzlich mit
[`goal.between_rounds`](goal-pauses.md) wiederholt zwischen abgeschlossenen
Runden warten. Frühere Aktivierungen bleiben als Revisionen erhalten;
`scheduled_after` liefert jeweils die letzte Revision einer Aufgabe.
`scheduled_history(sequence, after_revision, limit)` liest ihre Historie.

## Wiederkehrende Prüfungen

Eine eigenständige Reasoning-Route kann für jede passende Eingabeversion eine
Prüfserie anlegen. Ergänze dafür beispielsweise in der Route unter `routes`
in der [Rollen-Konfiguration](../config/roles/runtime.json) das Feld
`recurrence` und verwende für die geänderte Konfiguration eine eigene Runtime-ID
und ein eigenes Journal.

```json
{
  "recurrence": { "every_ms": 604800000, "max_occurrences": 16 }
}
```

Der erste Auftrag wird ab Aufnahme normal zur Ausführung eingeplant. Die
übrigen Termine liegen auf einem festen Raster ab diesem Zeitpunkt. `every_ms`
ist ein Abstand in Millisekunden von 1 bis 315576000000. `max_occurrences` zählt
alle 2 bis 10000 Rasterpositionen einschließlich der ersten. Die Route braucht
eine leere `allowed_capabilities`-Liste und verwendet weder `activation` noch
`goal`, `plan` oder eigene Abhängigkeiten. Reasoning-Budgets und eine optionale
Aufgabenfrist gelten pro ausgeführtem Termin.

Zwischen den Prüfungen belegt die Serie keinen Queueplatz. Ist eine Prüfung
noch offen, wartet die nächste. Sobald ein Platz frei ist, wird der jüngste
noch offene fällige Termin aufgenommen. Nach langer Standzeit kann das auch
der letzte konfigurierte Termin sein. Frühere fällige Termine gelten als
übersprungen. Sie zählen zum Terminlimit, und die nächste Fälligkeit bleibt
auf dem ursprünglichen Raster. Neue Eingaben teilen sich den verfügbaren
Platz mit fälligen Prüfungen. Bei Konkurrenz wechselt die bevorzugte Aufnahme
auch über Neustarts hinweg.

Der Host-Bericht enthält die anstehenden Serien unter `recurring_checks`
sowie die nächste Fälligkeit unter `next_wake_unix_ms`. `last_sequence` verweist
auf die zuletzt aufgenommene Aufgabe. `last_state` zeigt deren Status. Das
Tick-Ergebnis führt neu aufgenommene Folgetermine unter `recurring_jobs`.
`TaskRuntime::recurring_checks_after(sequence, limit)` liest Serien und
`TaskRuntime::check_occurrences_after(series_sequence, after_sequence, limit)`
liest aufgenommene Termine, jeweils bis zu 1000 pro Seite. Ein Terminbeleg
enthält geplante Fälligkeit, Aufnahmezeit und die Zahl der übersprungenen
Termine davor. Dieselben Angaben stehen unter `task_recurrence` im dauerhaft
gespeicherten Reasoning-Request. Die Prüfungen verwenden dieselbe
Eingabeversion. Eine konfigurierte Memory-Policy kann jeder Prüfung passenden
Kontext bereitstellen.

### Ganze Prüfserien steuern

Für `--series-command` wird die `sequence` der Serie aus `recurring_checks`
verwendet. Das ist die Sequenz der ursprünglichen Prüfung. `expected_revision`
entspricht der angezeigten `control_revision` und beginnt bei 0.

```json
{
  "sequence": 1,
  "expected_revision": 0,
  "command_id": "pause-checks-1-for-maintenance",
  "operator": { "actor_type": "human", "id": "operator.simon" },
  "reason": "Prüfungen während der Wartung pausieren",
  "change": "pause"
}
```

```bash
./target/debug/arcs-host --config host.json --runtime runtime.json \
  --series-command series-command.json
```

Die CLI übernimmt die Änderung bei gestoppter Runtime und gibt ihren Beleg
unter `series_control` aus. Im laufenden Host steht die lokale Monitorsteuerung
zur Verfügung. Eine Anwendung, die den Host besitzt, kann alternativ
`TaskRuntime::control_check_series` zwischen Ticks aufrufen. Für die
nächste Änderung wird eine neue `command_id` mit der aktuellen Revision
verwendet. Dieselbe unveränderte Anweisung liefert auch nach einem Neustart
ihren ursprünglichen Beleg. `series_control_history` liest die Belege mit
Betreiber und Begründung seitenweise. [Steuerung im laufenden Host](task-runtime.md#steuerung-im-laufenden-host)

- `pause` verhindert neue Termine und hält die noch offene Prüfung an.
  Eine laufende Reasoning-Antwort kann gespeichert werden. Eine Aufgabe
  behält ihre Frist und ihren Queueplatz, solange sie offen ist.
- `resume` setzt die Serie auf dem ursprünglichen Raster fort. Für verpasste
  Termine gilt weiterhin die Regel zum jüngsten fälligen Termin. Eine
  unabhängig gesetzte Pause der Einzelaufgabe bleibt wirksam.
- `cancel` beendet die Serie endgültig und bricht eine noch offene Prüfung
  mit ab. Bereits abgeschlossene Prüfungen und ältere Fehlerbelege bleiben
  im Verlauf. Ein schon gestarteter Worker kann bis Antwort oder Frist noch
  Ressourcen belegen. Eine verspätete Antwort öffnet die Aufgabe nicht wieder.

Der Serienstatus lautet `active`, `paused`, `cancelled` oder `exhausted`.
`exhausted` bedeutet, dass alle Termine verarbeitet sind und keine Prüfung
mehr offen ist. `last_state` zeigt separat den Ausgang der letzten Prüfung.
Eine pausierte Serie kann ihren geplanten nächsten Rastertermin anzeigen.
Sie zählt während der Pause nicht zu `next_wake_unix_ms`.

Unterbrechung laufender Planschritte, Kalender-/Cron-Termine, dynamisch durch
Reasoner vorgeschlagene Wartezustände, Abhängigkeitsplanung und ein allgemein
versioniertes Langzeit-Zielmodell bleiben offen. Die Architektur und ihre
Gesamtabnahme sind damit weiterhin nur teilweise umgesetzt.

```bash
cargo test --locked --offline -p arcs-host --test scheduled_runtime --test goal_runtime
```

Die Tests nutzen echte Adapterprozesse. Sie prüfen simulierte Monate,
Neustarts, feste relative Fälligkeit, vollständige Queue, Input- und
Observe-Weckereignisse, alte und unpassende Signale, begrenzte Logseiten,
persistente Fairness, Recovery nach gespeicherten Ergebnissen, bestehende
Journale und weiterhin erforderliche manuelle Aktionsfreigaben.
