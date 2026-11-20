# Parallele Teilaufgaben und gemeinsame Voraussetzungen

Dieser Schritt setzt Teile von [ARCHITECTURE.md](../../../docs/ARCHITECTURE.md)
§5.2 HET-03, HET-04 und HET-09, §5.3 und §7.2 PAR-03 um. Routen derselben
unveränderlichen Core-Eingabe können einen festen Abhängigkeitsgraphen bilden.
Unabhängige Zweige nutzen die vorhandenen begrenzten Reasoning-Worker. Eine
abhängige Aufgabe wartet ohne eigenen Worker auf alle Voraussetzungen.

## Startbares Beispiel

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/dependencies/host.json \
  --runtime apps/adapter-host/config/dependencies/runtime.json \
  --ticks 20
```

Ein Solver und der vorhandene aufgezeichnete Modell-Testadapter arbeiten
parallel an derselben Eingabe. Sie schlagen die Werte 2 beziehungsweise 1
vor. Eine dritte externe Rules-Capability erhält anschließend ausschließlich
deren `value`-Felder und liefert `{"sum":3.0,"count":2}` als eigenen Kandidaten.
Die Addition liegt im Adapter, nicht im allgemeinen Host oder Core. Der
Modell-Testadapter ist weiterhin keine echte LLM-Inferenz.

Der Bericht zeigt drei Ergebnisse zur Prüfung und keine wartende
Abhängigkeit mehr. Es entstehen keine freigegebenen Geräteaktionen.
Journale liegen getrennt unter `var/dependencies/`. Ein weiterer Start
übernimmt vorhandene Ergebnisse und führt dieselben Berechnungen nicht erneut aus.

```text
                         ┌─ nearest-solver ─┐
eine Core-Eingabe ────────┤                  ├─ sum-proposals
                         └─ nearest-model ──┘
```

## Konfiguration

Eine Route kann eine optionale `dependencies`-Liste ergänzen:

```json
"dependencies": [
  {"route_id": "nearest-solver", "payload_fields": ["value"]},
  {"route_id": "nearest-model", "payload_fields": ["value"]}
]
```

Es gilt eine **All-Regel**: Alle genannten Voraussetzungen müssen erfüllt
sein. Die Reihenfolge der Routen in der Datei ist frei; die Reihenfolge der
Abhängigkeiten bestimmt die dauerhafte Bindung der Ergebnisse. Selbstbezüge,
Zyklen, unbekannte oder doppelte Routen werden vor Öffnen des Runtimejournals
abgewiesen. Jede Route darf höchstens 31 Voraussetzungen benennen. Eine
Feldauswahl enthält 1–64 eindeutige Namen mit jeweils höchstens 128 Bytes.

Alle verbundenen Routen müssen dieselbe Input-/Observe-Capability und dasselbe
Eingabeschema verwenden. Zur Laufzeit werden sie zusätzlich an **dieselbe
Core-Eingabeversion** gebunden. Ein gleicher Subject-Text, ein ähnlicher
Auftrag oder ein Ergebnis einer anderen Eingabe ersetzt diese Bindung nicht.

Ohne `dependencies` bleibt das bisherige Verhalten erhalten. Leere Listen
werden nicht in die gespeicherte Routenidentität aufgenommen. Eine neue oder
geänderte Abhängigkeit verändert dagegen die fachliche Route und benötigt
eine neue Runtime-ID und ein neues Journal.

## Was erfüllt eine Voraussetzung?

Bei einer reinen Reasoning-Route muss ein gespeichertes Ergebnis mit
`solved` und mindestens einem schema-validierten Kandidaten vorliegen.
Alle Kandidaten werden als getrennte Kontextartefakte mit der ausdrücklich
konfigurierten Feldauswahl weitergegeben. `ready_for_review` allein reicht
nicht: `no_solution` und `budget_exhausted` erfüllen keine Voraussetzung.

Bei einer Goal-Route muss das Ziel tatsächlich `completed` sein. Ihr
abschließender Assessment-Kandidat wird mit den ausgewählten Feldern
weitergegeben, beispielsweise `goal_status` und `effect`. Weder ein geplanter
Schritt noch eine erteilte Freigabe oder ein bloßer Ausführungsbeleg zählt
als Zielabschluss. Ein evidenzbasiert bereits erfülltes Ziel kann wie bisher
auch ohne eigene Geräteaktion abgeschlossen werden.

Der Erhalt von Vorschlägen ist eine Datenvoraussetzung und keine Aussage,
dass sie wahr oder für eine physische Handlung freigegeben sind. Herkunft,
Trust, Versionsbindung und die bestehenden Sicherheitsprüfungen bleiben
erhalten. Ein nachfolgender Handlungskandidat muss seine konfigurierten
Abhängigkeitsartefakte als Herkunftsbelege referenzieren und erneut
Verification sowie eine eigene Approval bestehen. Eine Freigabe der
übergeordneten Aufgabe wird nicht an den Nachfolger übertragen.

## Persistenz, Fehler und Budgets

Alle zu einer Eingabe gehörenden Routenjobs, ihre Abhängigkeitskanten, anfängliche
Terminbedingungen und der Discovery-Cursor werden gemeinsam committed.
Darum kann eine volle Queue keinen halben Graphen annehmen. Die bestehende
Kapazitätsregel verlangt mindestens so viele Queueplätze wie konfigurierte
Routen. Wartende Teilaufgaben zählen zu `max_pending_jobs`.

Ein eigener dauerhafter Umlaufcursor begrenzt die Prüfung auf höchstens
`max_jobs_per_tick` wartende Zusammenführungen je Tick. Pro Prüfung werden
höchstens 31 Voraussetzungen gelesen. Auch ein länger wartender Zweig hindert
die übrigen Voraussetzungen und unabhängigen Aufgaben nicht am Fortschritt.

Sind alle Ergebnisse vorhanden, speichert die Runtime ihre unveränderlichen
Core-Versionen und Feldauswahlen atomar. Erst danach darf der Nachfolger
starten. Die Bindung bleibt bei Recovery unverändert und wird jedem seiner
Reasoning-Aufträge als Pflichtkontext hinzugefügt. Optionaler Memory-Abruf
verwendet weiterhin seine vorhandenen Scopes und eingefrorenen Kontextbelege.

Die tatsächlichen Eingänge sind außerdem unter
`constraints.task_dependencies` nach Elternroute benannt. Jeder Eintrag
enthält Elternauftrag, Ergebnisversion und die ausgewählten Kontextversionen.
Damit lassen sie sich von optionalen Erinnerungen mit demselben Schema
unterscheiden. Der Summenadapter verwendet ausschließlich diese gebundenen
Eingänge. Der Schlüssel ist für die Runtime reserviert: Objekt-Constraints
behalten ihre übrigen Felder; andere JSON-Werte stehen bei abhängigen
Reasoning-Routen unter `operator_constraints`. Das bestehende `goal_runtime`
von Goal-Aufträgen bleibt erhalten.

Ein Join reserviert im Kontextbudget Platz für den Eingang und die maximal
zulässige Kandidatenzahl aller Voraussetzungen. Goal-Routen reservieren
zusätzlich Platz für ihre Quellen und Wirkungsbewertung. Die tatsächlichen
Byte- und Ressourcenbudgets setzt weiterhin der Core durch. Fehlende
ausgewählte Ergebnisfelder blockieren den Nachfolger vor einem Modellaufruf.

Fehlgeschlagene Voraussetzungen blockieren ihre Nachfolger. Das wird
entlang weiterer Abhängigkeiten fortgesetzt. Unabhängige Zweige bleiben
ausführbar. Auch abgelehnte, abgebrochene oder budgetbeendete Goals und
`no_solution` erfüllen keine Voraussetzung.

Geblockte Nachfolger belegen keine weitere Queuekapazität. Sie werden nicht
automatisch erneut geplant oder bei einem später geänderten Ablauf wieder
geöffnet. Fertige Eltern werden bei einem Fehler des Join-Commits oder bei
einer verlorenen Antwort des Nachfolgers nicht erneut ausgeführt. Die
bestehenden Invocation-Verträge von Core und Adaptern gelten weiter.

## Status und Architekturgrenzen

Berichte enthalten `waiting_dependency` und `blocked_dependencies`; das
Kontrollpanel zeigt diese Zahlen an. Eine noch nicht begonnene abhängige
Goal-Route bleibt zunächst ein wartender Auftrag. Ihr eigentlicher
Goal-Checkpoint entsteht erst, wenn ihre Voraussetzungen bereitstehen.
Blockierte Aufträge werden deshalb getrennt von bereits begonnenen,
blockierten Goals gezählt.

`TaskRuntime::dependencies_after(sequence, limit)` liefert 1–1000 dauerhafte
Join-Belege je Seite. Sie enthalten Zustand, Eingabeversion, Elternaufträge,
Ergebnisversionen, Feldauswahl und gegebenenfalls einen Blockierungsgrund.
Die Referenzen sind operativer Laufzustand desselben Core und Journals.
Die fachliche Graphdefinition liegt wie andere Routenregeln im gebundenen
Core-Definitionsartefakt; die tatsächlichen Kontexte und Herkunftsbelege
bleiben im gemeinsamen Artifact-Netz. Es entsteht keine zweite Agentensteuerung.

Dies ist ein **vorab konfigurierter Graph innerhalb einer Eingabe**.
Allgemeine dynamische Planartefakte, automatische Teilzielanlage, Neuplanung,
Any-/Quorum-Regeln, eingabeübergreifende Abhängigkeiten und ein allgemeiner
Abbruchport für noch nicht gestartete Aufträge bleiben offen. Die
Gesamtabnahme für Heterarchie und parallele Planung ist damit weiterhin teilweise.

```bash
cargo test --locked --offline -p arcs-host \
  --test task_dependencies --test parallel_runtime --test goal_runtime
```

Die Prozess-Tests prüfen echte parallele Gates, das Warten auf alle Ergebnisse,
Feldauswahl und Trust, getrennte Eingaben, mehrere Kandidaten, Fehlerfortpflanzung,
ungültige Graphen, feste Memory-Kontexte, Wiederaufnahme nach verlorener Antwort
und fehlgeschlagenem Commit sowie eigenständige Freigaben für abhängige Goals.
