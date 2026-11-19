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

`max_concurrent_reasoning: 2` erlaubt zwei externe Reasoning-Aufrufe gleichzeitig.
Die Option akzeptiert 1–32. Der Standard 1 behält für bestehende Konfigurationen
das synchrone Verhalten. Das Limit ist ein Arbeitsbudget und verändert weder
Runtime-ID noch die Bedeutung bereits gespeicherter Aufträge.

Ab 2 kann eine spätere Runde laufende Aufträge abholen und freie Kapazität für
andere Aufgaben verwenden. Core-Prüfungen und Artefaktänderungen bleiben im
Host-Thread. Worker übernehmen ausschließlich externe I/O und senden Antworten
über einen begrenzten Kanal zurück. Der vollständige Request-Kontext wird vor
dem externen Aufruf als Artefakt persistiert.

Jede Adapterinstallation besitzt eine exklusive Session. Zwei Aufträge an
denselben Adapter laufen nacheinander, während unabhängige Adapter parallel
arbeiten können. Für Binäreingaben werden auch die Quelladapter reserviert.
Beobachtungen anderer Installationen werden weiter abgeholt. Ein Multi-Rollen-
Adapter wird während seines Reasoning-Aufrufs nicht zusätzlich gepollt.

Die gleiche Ausführung wird für `checking`, `planning` und `assessing` der
[Goal-Runtime](goal-runtime.md) verwendet. Verification, Approval und die
separate frische Wirkungsbeobachtung bleiben erforderlich.

Im parallelen Modus beendet ein Reasoner-Fehler nur den betroffenen Auftrag.
Er bleibt als `failed` gespeichert und wird nicht automatisch erneut versucht.
`failed_jobs` zeigt die Anzahl; `TickReport.failed` enthält die betroffenen
Aufgaben und Fehler. Fehlgeschlagene Aufgaben zählen nicht zur aktiven
Queue-Kapazität. Ausgefallene Sessions werden bis zum nächsten Hoststart
ausgespart. Store- und Invocation-Konsistenzfehler werden weiterhin weitergegeben.

Ein Stop beendet ausstehende lokale Adapterdialoge und wartet nicht deren
vollständiges Reasoning-Budget ab. Ein bereits extern gespeichertes Ergebnis
kann beim Neustart mit derselben Invocation-ID wiederaufgenommen werden.
Der Adapter muss die entsprechende dauerhafte Deduplizierung gewährleisten.
Ein gestoppter lokaler Prozess garantiert keine Rücknahme einer entfernten
Berechnung oder einer bereits eingetretenen Wirkung.

Dieser erste Slice ließ Request, Act, Output und das Polling synchron.
Der spätere [Request-Ausbau](parallel-requests.md) ergänzt begrenzt parallele
Goal-Quellenabfragen; Act, Output und das Polling bleiben weiterhin synchron.
Geräteübergreifende Ressourcenidentitäten, GPU-/RAM-Budgets, parallele
Planverzweigungen und automatische Wiederverbindungen sind noch offen.
Eine gemeinsame Drohne hinter zwei verschiedenen Installationen wird noch
nicht durch die Session-Reservierung geschützt.

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
