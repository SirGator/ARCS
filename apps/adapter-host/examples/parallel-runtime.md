# Begrenztes paralleles Reasoning

Der erste Parallelitätsnachweis nutzt die bestehende Aufgabenqueue und denselben
Core-Store. Die Konfiguration verbindet eine JSON-Eingabe, einen mathematischen
Solver und einen aufgezeichneten Modell-Testadapter. Drei Routen erzeugen drei
unabhängige Aufgaben aus einer Eingabe; zwei Routen teilen sich den Solver.
Modell-Testantworten sind keine echte LLM-Inferenz.

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/parallel/host.json \
  --runtime apps/adapter-host/config/parallel/runtime.json \
  --ticks 20
```

Bei einer neuen Installation meldet der Bericht drei Ergebnisse zur Prüfung.
Kandidaten sind keine freigegebenen Aktionen. Journale liegen getrennt unter
`var/parallel/`. Ein erneuter Start übernimmt fertige Ergebnisse ohne erneuten
Reasoner-Aufruf. Für neue Aufgaben kann derselbe Input-Adapter verwendet werden:

```bash
./target/debug/arcs-json-input --enqueue \
  apps/adapter-host/config/parallel/input-profile.json \
  var/parallel/input.sqlite \
  apps/adapter-host/config/parallel/input-live-event.json
```

## Ausführung und Grenzen

`max_concurrent_reasoning` begrenzt externe Reasoning-Aufrufe auf 1–32.
Der Standardwert 1 erlaubt einen laufenden Modellauftrag. Der Scheduler
verarbeitet in der Zwischenzeit Steuerbefehle, neue Eingaben, Beobachtungen
und verfügbare Schritte anderer Manager. Größere Werte erlauben mehrere
Reasoning-Aufrufe, sofern ihre Adapter und Ressourcen verfügbar sind.
Das Limit verändert die Identität gespeicherter Aufträge nicht.

Der Host speichert die vollständige Invocation vor dem externen Aufruf im
Core. Worker übertragen die Anfrage und liefern ihre Antwort über einen
begrenzten Kanal. Der Host prüft und speichert die Antwort in einem späteren
Tick.

Jede Adapterinstallation besitzt eine exklusive Session. Zwei Aufträge an
denselben Adapter laufen nacheinander, während unabhängige Adapter parallel
arbeiten können. Für Binäreingaben werden auch die Quelladapter reserviert.
Beobachtungen anderer Installationen werden weiter abgeholt. Ein Multi-Rollen-
Adapter wird während seines Reasoning-Aufrufs nicht zusätzlich gepollt.

Die gleiche Ausführung wird für `checking`, `planning` und `assessing` der
[Goal-Runtime](goal-runtime.md) verwendet. Verification, Approval und die
separate frische Wirkungsbeobachtung bleiben erforderlich.

Ein Modellfehler beendet den betroffenen Auftrag. `failed_jobs` zeigt die
Anzahl, `TickReport.failed` die zugehörigen Aufgaben und Fehler. Der Auftrag
gibt Queuekapazität frei. Ein gespeicherter Modellfehler wird nach einem
unterbrochenen Journal-Commit aus dem Core übernommen. Für ausdrücklich
autorisierte Alternativen gelten die gespeicherten Recovery-Regeln.
Store- und Invocation-Konsistenzfehler werden an den Aufrufer weitergegeben.

Ein Stop beendet ausstehende lokale Adapterdialoge und wartet nicht deren
vollständiges Reasoning-Budget ab. Ein bereits extern gespeichertes Ergebnis
kann beim Neustart mit derselben Invocation-ID wiederaufgenommen werden.
Der Adapter muss die entsprechende dauerhafte Deduplizierung gewährleisten.
Ein gestoppter lokaler Prozess garantiert keine Rücknahme einer entfernten
Berechnung oder einer bereits eingetretenen Wirkung.

Die direkten Host-Aufrufe `reason` und `reason_with_data` sind synchrone
Einzelaufrufe. Der Request-Pool, die Gerätesitzungen und die gemeinsamen
Ressourcenbudgets sind in den jeweiligen Beispielen beschrieben.
[Quellenabfragen](parallel-requests.md),
[Ressourcenpools](resource-pools.md)

## Deterministischer Parallelitäts- und Recovery-Test

```bash
cargo test --locked --offline -p arcs-host --test parallel_runtime
cargo test --locked --offline -p arcs-host --test goal_runtime
```

Die Tests verzögern Antworten über Dateiereignisse. Zwei unabhängige Aufgaben
und Beobachtungen müssen währenddessen fortschreiten. Weitere Fälle prüfen
Session- und Worker-Limits, Komponentenfehler, ungültige Antworten, Timeouts und
einen Neustart nach extern gespeichertem Ergebnis vor der Core-Übernahme.
Goal-Tests prüfen auch freigegebene Aktionen und Wiederaufnahme ohne zweite Wirkung.

Den verbindlichen weiteren Ausbau beschreibt der
[Architekturvertrag](../../../docs/ARCHITECTURE.md).
