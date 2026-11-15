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
