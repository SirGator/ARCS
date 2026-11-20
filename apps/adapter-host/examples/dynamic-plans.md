# Dynamische Pläne aus freigegebenen Vorlagen

Dieser Schritt erweitert [ARCHITECTURE.md](../../../docs/ARCHITECTURE.md),
§5.2 HET-01 bis HET-04 sowie HET-08/HET-09, §5.3 und §8.2 LNG-02:
Ein externer Reasoner schlägt einen begrenzten Teilplan vor. Die Runtime
prüft dessen Vorlagen, Abhängigkeiten, Feldauswahl und Gesamtbudgets,
speichert einen unveränderlichen Plan im gemeinsamen Artifact-Netz und
führt seine Schritte mit der vorhandenen Aufgabenqueue aus. Neben reinem
Reasoning sind [Goal-Vorlagen mit manueller Aktionsfreigabe](goal-plans.md)
möglich.

## Startbares Beispiel

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/dynamic-plans/host.json \
  --runtime apps/adapter-host/config/dynamic-plans/runtime.json \
  --ticks 20
```

Der externe `plan_nearest`-Rules-Reasoner entscheidet anhand der Eingabe:
Bei mehreren Zahlen schlägt er einen Solver, einen aufgezeichneten
Modell-Testadapter und eine anschließende Summierung vor. Die ersten beiden
Schritte laufen unabhängig; der dritte erhält ihre ausgewählten `value`-Felder.
Der Summenkandidat enthält `{"sum":3.0,"count":2}`.

Der Bericht zeigt einen Plan mit `ready_for_review` und vier Review-Einträge:
den ursprünglichen Planvorschlag sowie die drei Schrittergebnisse. Es sind
keine vier abgeschlossenen realen Ziele und keine freigegebenen Aktionen.
Journale liegen getrennt unter `var/dynamic-plans/`.

Eine neue Eingabe mit nur einer Zahl erzeugt einen anderen Plan:

```bash
./target/debug/arcs-json-input --enqueue \
  apps/adapter-host/config/parallel/input-profile.json \
  var/dynamic-plans/input.sqlite \
  apps/adapter-host/config/dynamic-plans/single-event.json
```

Danach denselben Host-Aufruf erneut ausführen. Der zweite Plan enthält nur
einen Solver-Schritt, dessen Kandidat den Wert 9 liefert. Insgesamt gibt es
nun zwei Pläne und sechs Review-Einträge. Weitere Starts wiederholen fertige
Pläne und Schritte nicht. Die Demo benutzt deterministische Regeln und einen
aufgezeichneten Modell-Testadapter; sie behauptet keine echte LLM-Inferenz.
Ein kompatibler externer Reasoner kann denselben Planvertrag verwenden.

## Betreiberkonfiguration und Modellvorschlag

Die Root-Route verwendet `arcs.plan_candidate.v1` als Ziel-Schema, genau einen
Kandidaten und eine optionale `plan`-Konfiguration:

```json
"plan": {
  "max_steps": 3,
  "max_total_runtime_ms": 3000,
  "templates": [
    {
      "route": {"id": "solver", "…": "vollständige ReasoningRoute"},
      "export_fields": ["value"]
    }
  ]
}
```

Der Ausschnitt erläutert die Struktur; die vollständigen Verträge stehen in
[`runtime.json`](../config/dynamic-plans/runtime.json). Jede Vorlage enthält
eine normale, vollständig konfigurierte Reasoning-Route: Quelle, Eingabeschema,
ausgewählte Eingangsfelder, Reasoner-Capability, Methode, Objective, Ziel-Schema,
Constraints, Budget und gegebenenfalls Memory und anfängliche Aktivierung.
Alle Capabilities und Budgets werden bei der Einrichtung und vor jedem Tick
gegen die aktuellen registrierten Manifeste und Grants geprüft, auch für
noch nicht ausgewählte Vorlagen.

Der Planner erhält unter `constraints.task_planning` einen Katalog mit
Vorlagen-ID, Objective, Reasoner, Methode, Ziel-Schema, Eingangs-/Exportfeldern
und Ressourcenbudget. Goal-Vorlagen beschreiben zusätzlich ihre Runden-,
Assessment- und manuellen Freigabegrenzen sowie ihr Ergebnis-Schema.
Rohe Vorlagen-Constraints, Abfrage-Payloads und Memory-Konfigurationen
werden nicht in diesen Katalog kopiert. Die vollständige Vorlage bleibt
Betreiberkonfiguration und erhält erst ihr beauftragter Schritt. Der Schlüssel
`task_planning` ist für die Runtime reserviert; ursprüngliche Nicht-Objekt-
Constraints stehen unter `operator_constraints`.

Der Modellvorschlag darf ausschließlich diese Form haben:

```json
{
  "steps": [
    {"id": "exact", "template": "solver"},
    {"id": "alternative", "template": "model"},
    {
      "id": "combined",
      "template": "sum",
      "dependencies": [
        {"step_id": "exact", "payload_fields": ["value"]},
        {"step_id": "alternative", "payload_fields": ["value"]}
      ]
    }
  ]
}
```

Der Planner darf Vorlagen auswählen, mehrfach instanziieren und verknüpfen.
Er darf keine Programme, Argumente, Zugangsdaten, Quellen, Objectives,
Capabilities, Grants oder Budgets überschreiben. Jede Kanten-Feldauswahl muss
eine Teilmenge der `export_fields` der jeweiligen Elternvorlage sein.
Die spätere Ergebnisbindung und die benannten Join-Eingänge verwenden den
bereits vorhandenen [Abhängigkeitsvertrag](task-dependencies.md).

## Grenzen vor der Aufnahme

- 1–31 Schritte je Plan und 1–32 freigegebene Vorlagen je Planerroute.
- Eindeutige Schritt-IDs mit 1–32 ASCII-Zeichen gemäß dem bisherigen
  Bezeichnervertrag; Vorlagen-IDs sind normale Routen-IDs mit höchstens 64 Bytes.
- Nur Referenzen auf Schritte desselben Vorschlags; keine Zyklen, Selbstbezüge
  oder doppelten Voraussetzungen. Alle Schritte bleiben an dieselbe originale
  Core-Eingabeversion gebunden.
- Reine Reasoning-Vorlagen haben keine `allowed_capabilities`. Goal-Vorlagen
  verwenden die bestehende Zielschleife mit `approval.mode: manual` und brauchen
  eine ausdrückliche `max_total_actions`-Grenze. Automatische Policy-Freigaben,
  verschachtelte Pläne und feste Vorlagen-Abhängigkeiten sind ausgeschlossen.
  Die Root-Route besitzt weiterhin keine direkte Action-Autorität.
- Der bestehende Kontextvertrag muss alle maximal möglichen Elternkandidaten
  aufnehmen können. Byte-, Ausgabe- und Laufzeitlimits setzt weiterhin der Core durch.
- Ein angenommener Plan-Snapshot darf höchstens 60.000 serialisierte Bytes enthalten.

Die Form wird durch das gebündelte Schema geprüft; die genannten oberen
Größen-, Graph- und Autoritätsgrenzen prüft zusätzlich die Runtime. Diese
Prüfung ergänzt den bewusst begrenzten JSON-Schema-Validator des Projekts.

`max_total_runtime_ms` begrenzt die Summe der maximal erlaubten Rechenzeiten
aller ausgewählten Schritte, bis höchstens 111.600.000 ms. Bei Goals zählen
alle möglichen Runden einschließlich Planung, Wirkungsbewertung und optionalem
anfänglichen Zielcheck. Das ist eine
konservative Obergrenze für Reasoning, keine Kalenderfrist: Eine konfigurierte
Terminpause darf länger dauern. Der erste Planner-Aufruf hat sein eigenes
normales Routenbudget.

Optional begrenzt `max_total_output_tokens` die Summe der maximalen Modell-
Ausgabetokens der ausgewählten Schritte einschließlich aller Goal-Planungs-
und Bewertungsaufrufe. Ist diese Grenze gesetzt, werden
Modellschritte ohne belegbare `max_output_tokens`-Obergrenze abgewiesen.
Ohne sie gelten weiterhin die einzelnen Vorlagenbudgets. Geldbudgets,
allgemeine Eingabetokenabrechnung und eine kalenderbezogene Gesamtfrist
sind dadurch noch nicht implementiert.

`max_total_actions` begrenzt die Summe der maximalen Goal-Runden und damit die
maximal geplanten Action-Dispatches. Sobald der Katalog Goal-Vorlagen enthält,
ist die Grenze Pflicht; erlaubt sind 0–31.000. Jede tatsächlich ausgeführte
Aktion braucht zusätzlich ihre eigene Verification und konkrete Freigabe.

## Ein Plan-Artefakt, eine Runtime

Der nicht vertrauenswürdige Vorschlag bleibt als `arcs.plan_candidate.v1` samt ursprünglichem
Core-Reasoning-Auftrag erhalten. Nach bestandener Prüfung erzeugt der Core-
Publisher `arcs.host.plan` ein `arcs.plan.v1` mit Eingabe-/Vorschlagsversion,
Root-Identität und vollständig eingefrorenen Schrittrouten. Typisierte Kanten
binden es an Vorschlag, Eingabe und bestehendes Runtime-Definitionsartefakt.

Die Publisher-Identität ist für externe Adapter gesperrt. Das Plan-Artefakt
behält ausdrücklich `low` Trust: Eine zulässige Topologie erhöht nicht die
Wahrheit von Modellinhalten und ist keine Action-Approval. Neue Berechtigungen
entstehen weder durch die Planung noch durch die Aufnahme eines Schritts.
Bei Wiederaufnahme werden die Schrittrouten gegen Vorschlag und unveränderte
Betreiber-Vorlagen geprüft.

Anschließend nimmt dieselbe Runtime ihre Kindaufträge und Abhängigkeiten
in einer Journaltransaktion auf. Scheitert sie nach der Core-Veröffentlichung,
bleibt der Plan-Snapshot erhalten, aber es entsteht kein halber ausführbarer
Graph. Die Wiederaufnahme verwendet denselben Vorschlag und Snapshot und
trägt die Aufnahme nach. Eine erneute Planung erfolgt dabei nicht.

## Kapazität, Wartezustände und Fehler

Für jede angenommene Planeraufgabe werden bereits bei ihrer Entdeckung
`1 + max_steps` Queueplätze reserviert. Daher muss `max_pending_jobs` mindestens
der Summe aller festen Routenplätze und Planerreservierungen entsprechen.
Bei der Aufnahme ersetzen die tatsächlichen Kindaufträge die reservierten
Plätze; ungenutzte Plätze werden frei. Mehrere offene Planer können sich so
nicht gegenseitig sämtliche Plätze für ihre spätere Erweiterung wegnehmen.

`pending_jobs` zählt weiterhin tatsächliche offene Aufträge. `capacity_used`
enthält zusätzlich reservierte Plätze und wird für Backpressure verwendet.
Normale Routen ohne Plan zählen wie bisher jeweils als ein Platz. Die
vorhandene maximale Reasoning-Parallelität und exklusive Adapter-Sessions
gelten unverändert. Schlafende Schritte belegen keine Reasoning-Worker.

Anfängliche Termin-/Ereignisbedingungen und erlaubte Memory-Scopes einer
Vorlage bleiben Bestandteil des Schritts. Fälligkeiten werden bei der atomaren
Aufnahme gespeichert und über Neustarts erhalten. Die eigentlichen
Invocation-IDs, Ergebnisbindungen und Memory-Kontexte folgen den vorhandenen
Recovery-Verträgen. Ein Backend muss weiterhin seinen Invocation-Vertrag
erfüllen, um externe Aufrufe zuverlässig zu deduplizieren.

Ein formal gültiger, aber unzulässiger Vorschlag erhält `rejected` mit
Begründung und erzeugt keine Kindaufträge. Formal fehlerhafte Antworten werden
bereits als normaler Reasoning-Fehler abgewiesen. `no_solution` erzeugt keinen
ausführbaren Plan. Misslingt später ein benötigter Schritt, wird der Plan
`blocked`; seine abhängigen Nachfolger werden ebenfalls blockiert. Bereits
unabhängig gestartete Berechnungen dürfen fertig werden. Für eine weitere Planung braucht der Auftrag eine eigene Replanning-Policy.

## Status und noch offene Arbeit

`TaskRuntime::plans_after(sequence, limit)` liefert 1–1000 Planbelege pro Seite:
Status, Eingabe-/Vorschlags-/Planversion, Kindaufträge und ihre Zustände sowie
gegebenenfalls den Ablehnungs- oder Blockierungsgrund. Der Runtime-Bericht
enthält zusätzlich `running_plans`, `stopped_plans`, `review_plans` und bis zu
50 aktive beziehungsweise zuletzt angelegte Planbelege. Das Kontrollpanel
zeigt aktive und angehaltene Teilpläne an.

## Einen Plan gezielt abbrechen

Die Host-Anwendung kann `TaskRuntime::cancel_plan(&mut host, sequence, reason)`
aufrufen. `sequence` ist die ID des Planner-Root-Auftrags aus dem Bericht.
Vor dem ersten Vorschlag steht sie unter `planning_jobs`, danach unter `plans`.
`planning_jobs_after(sequence, limit)` liefert die noch planenden Root-Aufträge
in Seiten von 1–1000 Einträgen; der Bericht zeigt davon höchstens 50.
Damit lässt sich auch ein noch schlafender oder auf seine erste Antwort
wartender Planner abbrechen. Kindaufträge und gewöhnliche Routen werden über
diesen Einstieg nicht einzeln verändert.

Für eine gestoppte Runtime steht derselbe Vorgang über die CLI bereit:

```json
{"command":"cancel","sequence":1,"reason":"Projekt nicht weiter bearbeiten"}
```

Diese Datei als `cancel-plan.json` speichern und ausführen:

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/dynamic-plans/host.json \
  --runtime apps/adapter-host/config/dynamic-plans/runtime.json \
  --plan-command cancel-plan.json
```

Ein laufender CLI-/Monitor-Prozess muss zuvor über seine Stop-Datei beendet
werden: Das Journal bleibt exklusiv gesperrt. Der Aufruf ist kein zweiter
Steuerungsprozess und kein Live-IPC-Kanal. Er führt keine weiteren Ticks aus.
`--goal-command`, `--ticks`, `--monitor` und `--stop-file` sind dabei ausgeschlossen.

Der operative Abbruchbeleg, der Root-Status und alle noch offenen Kindaufträge
werden atomar gespeichert. Wartende Joins werden blockiert. Bereits fertige
Review-Ergebnisse, Plan-Snapshots, Eingabeversionen und Aktivierungshistorien
bleiben erhalten. Eine identische Wiederholung erhält denselben Beleg mit
unveränderter Begründung und Zeit. Eine neue Begründung überschreibt ihn nicht.
Ein bereits fertiger Plan wird abgewiesen; ein blockierter Plan mit noch
offenen Schritten kann deren Restarbeit abbrechen.

Der Planbeleg zeigt `cancelled`, `detail` und `cancelled_at_unix_ms`.
Bei Abbruch vor dem Vorschlag dürfen Vorschlags-/Planversion fehlen.
`stopped_plans` zählt diese Instanzen mit; `cancelled_jobs` zählt die abgebrochenen
Root- und Kindaufträge. Sie werden nicht als blockierte Goals gezählt.
Die additive Journaltabelle erhält bestehende Runtime-Identitäten. Dieser
operative Abbruchbeleg ist noch kein allgemeines versioniertes Core-Zielmodell;
das unveränderliche `arcs.plan.v1` wird dadurch nicht umgeschrieben.

Schon gesendete Berechnungen können noch Kosten verursachen. Ihre Worker
behalten bis zur Antwort oder ursprünglichen Deadline ihre Session- und
Parallelitätsplätze. Rechtzeitige Antworten werden verworfen und die Session
kann danach andere Aufgaben bedienen. Bei Timeout oder Transportfehler wird
nur die betroffene Session abgebrochen und bleibt nach dem bestehenden
Host-Vertrag bis zum Neustart nicht verfügbar. Späte Antworten erzeugen keine
Kandidaten oder Review-Ergebnisse des abgebrochenen Auftrags. Core-Invocations
ohne übernommene Antwort behalten ihren unbekannten Ausgang; ein Abbruch
erfindet weder einen Erfolg noch eine Rücknahme externer Wirkungen.

Unabhängige Pläne bleiben ausführbar. Abgebrochene Aufträge werden nach einem
Neustart, einem späteren Termin oder einem Weckereignis nicht erneut gestartet.
Bereits laufende Worker behalten ihre reservierte Kapazität bis zur Antwort
oder zum Abbruch an der Deadline.

## Grenzen der aktuellen Planung

`ready_for_review` bedeutet: Alle reinen Reasoning-Schritte lieferten
schema-valide `solved`-Kandidaten und alle Goal-Schritte erreichten den Zustand
`completed` mit ihrem Bewertungsbeleg. Die einzelnen Goals prüfen Wirkungen
über die bestehende Zielschleife; der Gesamtplan besitzt noch keinen eigenen
allgemeinen Zielprüfer. Die Root-
Fertigmeldung enthält weiterhin den ursprünglichen Planvorschlag; die
einzelnen Ergebnisse sind über die Kindaufträge lesbar. Andere feste Routen
können deshalb noch nicht vom Plan-Orchestrator als Ergebnislieferant abhängen.

Dies ist die erste **dynamische Planung von Reasoning-Aufgaben innerhalb
eines explizit freigegebenen Katalogs**, ergänzt um manuell freigegebene
Goal-Schritte. Direkte Act-Schritte außerhalb der Zielschleife, automatische
Freigaben in erzeugten Plänen, freie Werkzeugargumente, Neuplanung laufender Instanzen, rekursive Teilpläne,
Live-IPC-Steuerung und universelle Fähigkeitsentdeckung bleiben offen.
Unveränderte Routen ohne `plan` behalten ihre Identität; eine neue oder
geänderte Planer-/Vorlagenkonfiguration benötigt eine neue Runtime-ID und ein
neues Journal.

```bash
cargo test --locked --offline -p arcs-host \
  --test dynamic_plans --test task_dependencies --test plan_cancellation
```

Die Tests nutzen externe Test-Reasoner mit persistenten Antworten. Sie prüfen
parallele Erweiterung, verbotene Vorlagen/Felder und Gesamtbudgets, getrennte
Eingaben, volle Kapazität, verlorene Planner-/Schrittantworten, einen fehlgeschlagenen
Aufnahme-Commit, private Vorlagen-Constraints, gesperrte Publisher-Identität
und konfigurierte Wartephasen über einen simulierten Monat.
Die Abbruchtests prüfen zusätzlich späte Antworten, unveränderte fertige
Ergebnisse, idempotente CLI-Kommandos, additive Migration, volle Workerplätze,
ursprüngliche Deadlines, fremde Core-Stores und atomaren Rollback.

Die spätere [versionierte Neuplanung](plan-replanning.md) ergänzt optional
Ersatzvorschläge nach blockierten Schritten unter kumulativen Budgets.
