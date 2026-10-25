# Dauerhafte Aufgabenverarbeitung

`arcs-host --runtime` ergänzt den bisherigen begrenzten Host um eine dauerhafte,
synchrone Polling-Schleife. Neue Inputs und Beobachtungen werden anhand einer
expliziten Konfiguration Reasonern zugeordnet. Das Beispiel verwendet die
bestehende JSON-Eingabe und den mathematischen Solver. Fachliche Geräte- und
Solverlogik bleiben in den Adaptern.

```text
Input / Observe → Core-Log mit ACK
                       ↓ Leseposition + Aufgaben atomar
                 Runtime-Warteschlange
                       ↓ stabile Request-ID
                 Reasoning V2 → Kandidaten
                       ↓
                 ready_for_review
                       ↓ Anwendung: Verification → Approval → Act / Output
```

Ohne `goal` bietet die Route die persistente Übergabe an die Anwendung.
Optionale [Goal-Routen](goal-runtime.md) koordinieren auch die letzte Zeile,
frische Wirkungsdaten und erneute externe Planung. Ein Auftrag oder Kandidat
erteilt keine Freigabe; manuelle Entscheidung oder ausdrückliche Betreiberpolicy
bleiben erforderlich.

## Start und neue Aufgaben

Aus dem Workspace:

```bash
cargo build --locked --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/roles/host.json \
  --runtime apps/adapter-host/config/roles/runtime.json \
  --ticks 2
```

Dieser begrenzte Nachweis verarbeitet auch eine bereits früher empfangene und
bestätigte Aufgabe. Wiederholte Starts erzeugen keine weiteren Aufrufe für
fertige Aufgaben. Der Bericht enthält dauerhaft gespeicherte Zahlen für
`ready_for_review`, `pending_jobs`, `failed_jobs` und den `source_cursor`.
`ticks` zählt nur die Runden des aktuellen Aufrufs.

Für den Dauerbetrieb im ersten Terminal:

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/roles/host.json \
  --runtime apps/adapter-host/config/roles/runtime.json \
  --stop-file /tmp/arcs-nearest.stop
```

Der Stop-Pfad darf beim Start noch nicht existieren. Im zweiten Terminal lässt
sich während des laufenden Hosts eine Aufgabe hinzufügen:

```bash
./target/debug/arcs-json-input --enqueue \
  apps/adapter-host/config/roles/input-profile.json \
  var/roles-input.sqlite \
  apps/adapter-host/config/roles/input-live-event.json
```

Der neue Solver-Kandidat enthält den Wert **7**, den nächsten Wert zu **6,5**.
`--enqueue` speichert die Nachricht atomar im Adapterjournal. Gleiche ID und
Inhalt sind auch nach einem ACK wiederholbar; andere Inhalte unter derselben
ID werden abgewiesen. Für eine neue Aufgabe braucht die Ereignisdatei neue
stabile `id`- und `reference`-Werte. Das Profil bleibt unverändert.

Geordnetes Beenden:

```bash
touch /tmp/arcs-nearest.stop
```

Der Host prüft die Datei zwischen Runden und während der Pausen. Ein laufender
Dialog ist weiterhin durch seine Deadline begrenzt; die Stop-Datei unterbricht
keine laufende Runde. Deren gesamte Dialoganzahl und Deadlines bestimmen die
maximale Reaktionszeit. Danach beendet er die
Adapter und gibt seinen Bericht aus. Vor einem weiteren Dauerlauf muss die
von dir erzeugte Stop-Datei wieder entfernt werden. Ein harter Prozessabbruch
unterbricht den aktuellen Dialog; die gespeicherte Aufgabe bleibt wiederaufnehmbar.

## Konfiguration und Grenzen

Die Runtime-Konfiguration ist separat von der Anschlusskonfiguration des Hosts.
`journal` wird relativ zur Runtime-Datei aufgelöst. Relative Hostpfade bleiben
relativ zur Hostdatei; die CLI-Option `--stop-file` bezieht sich auf das aktuelle
Arbeitsverzeichnis. Absolute Konfigurationspfade erlauben Starts aus jedem
Verzeichnis. Der Aufruf ohne `--runtime` behält sein bisheriges Verhalten. Der Runtime-Modus
verarbeitet Input-/Observe-Routen mit optionaler Zielsteuerung; konfigurierte einmalige Request-/
Transform-Aufrufe des begrenzten Hosts werden dabei nicht automatisch ausgeführt.

| Feld | Bedeutung |
| --- | --- |
| `config_version` | Derzeit 1 |
| `runtime_id` | Dauerhafte Identität, 1–64 ASCII-Buchstaben, Ziffern, Punkt, Unterstrich oder Bindestrich |
| `journal` | Separate SQLite-Datenbank der Aufgabenwarteschlange |
| `interval_ms` | Pause zwischen Runden, 1–60.000 ms; Standard 100 |
| `max_events_per_tick` | Größe einer Core-Logseite, 1–10.000; Standard 1000; höchstens zwei Seiten je Runde |
| `max_jobs_per_tick` | Maximale bearbeitete Aufgaben/Schritte je Runde, 1–1000; Standard 100 |
| `max_pending_jobs` | Offene Aufgaben, mindestens Anzahl der Routen, höchstens 100.000; Standard 1000 |
| `routes` | 1–32 Zuordnungen mit eindeutigen IDs |

Eine Route nennt Quelle, Eingabeschema, ausgewählte Payloadfelder, Reasoner,
Verfahren, Ziel-Schema, Constraints, erlaubte Fähigkeiten und Ressourcenbudget.
Die Quelle kann `Input` oder `Observe` anbieten. Der Core setzt Herkunft und
Trust. `private_note` wird im Beispiel nicht an den Reasoner übergeben.
Mehrere Routen dürfen denselben Input unterschiedlichen Reasonern zuordnen.

Routing und tatsächliche Manifeste werden vor dem Polling geprüft. Die
Definition wird unter der Runtime-ID im Core gebunden. Geänderte Routen,
Budgets oder Manifeste brauchen eine neue Runtime-ID und ein neues Journal.
Polling-Intervall und Arbeitsbudgets lassen sich dagegen ändern. Aktuelle
Grants werden vor jeder Runde und jedem Reasoning-Aufruf erneut geprüft.

## Persistenz und Wiederaufnahme

Der Adapter hält eine Nachricht bis zum Core-ACK vor. Wird nach dem ACK der
Prozess unterbrochen, findet die Runtime das Ereignis später im Core-Log.
Leseposition und alle zugehörigen Routenaufgaben werden im Runtimejournal
in einer Transaktion gespeichert. Bei voller Queue wird der betroffene
Logeintrag noch nicht übersprungen. Die Runtime bremst den weiteren Empfang;
bereits bestätigte Ereignisse können im Core-Log auf ihre Übernahme warten.
Ein Polling-Batch des Hosts kann daher mehr Ereignisse speichern als sofort
in die begrenzte Aufgabenqueue passen.

Vor einem Reasoning-Aufruf wechselt die Aufgabe auf `running`. Die Request-ID
bindet Runtime, Route und Eingabeversion. Der Core und der konkrete Adapter
journalisieren denselben Aufruf. Falls die Antwort schon gespeichert ist,
aber die Queue noch nicht `ready_for_review` meldet, trägt der nächste Start
die Fertigmeldung mit denselben Artefaktversionen nach. Eine unabhängige
externe Wirkung kann ein Backend nur dann zuverlässig deduplizieren, wenn
es seinen bestehenden Invocation-Vertrag erfüllt.

Pro Runtimejournal besitzt ein Prozess eine Betriebssystem-Dateisperre. Die
Sperre wird auch nach einem Prozessabbruch freigegeben. Das Journal ist an
seinen kanonischen Pfad, den Core-Datenbankpfad und die Core-Definition gebunden;
es kann nicht an einen anderen Core angehängt werden. Core, Runtimejournal
und Adapterjournale müssen konsistent gesichert und wiederhergestellt werden.
Fertige Aufgaben bleiben als dauerhafte Historie gespeichert.

Fehler stoppen den Lauf sichtbar. Die aktuelle Aufgabe bleibt `failed` mit
Fehlertext; es gibt keine versteckte Wiederholungsschleife oder automatische
Neuverbindung. Beim ausdrücklich gestarteten nächsten Lauf entscheidet der
Core-Aufrufvertrag, ob Wiederaufnahme beziehungsweise Retry zulässig ist.
Ein permanenter Fehler wird nicht als Erfolg übersprungen.

## Übergabe an die Anwendung

`TaskRuntime::tick(&mut Host)` liefert neue Fertigmeldungen dieser Runde.
`completed_after(cursor, limit)` liest die dauerhafte Review-Liste in begrenzten
Seiten. Jede Meldung enthält Eingabe-, Request-, Ergebnis- und Kandidatenversionen.
Die Anwendung führt ihren eigenen Review-Cursor und verwendet die bestehenden
Verification-/Approval-Dienste. Der Review-Cursor muss erst nach deren eigener
idempotenter Speicherung fortgeschrieben werden. Auch `no_solution` und
`budget_exhausted` sind gespeicherte Ergebnisse mit leerer Kandidatenliste.

`TaskRuntime::run` unterstützt einen Stop-Callback und optional eine Rundenzahl.
Die Runtime hält Verbindungen offen und verarbeitet seriell. Sie umfasst kein
Push-Protokoll, keine parallele Reasoner-Ausführung, keine automatische Modellwahl
und keine implizite Action-/Output-Policy. Goal-Aktionen benötigen die oben
beschriebene ausdrückliche Konfiguration. Die Host-Inputberichte enthalten
im Dauerbetrieb nur die letzte Empfangsrunde statt einer unbegrenzten In-Memory-Liste.

```bash
cargo test --locked -p arcs-host --test task_runtime
```

Die Prozess-Tests prüfen ACK-/Queue-Grenzen, Live-Eingaben nach Leerlauf,
Wiederaufnahme nach gespeicherter Reasoning-Antwort, Fanout und Rückstau,
Observe als Quelle, Definitionstreue, exklusiven Betrieb und CLI-Stop/Neustart.
