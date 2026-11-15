# Goal-Schritte in dynamischen Plänen

Ein externer Planer kann neben reinen Reasoning-Schritten jetzt vollständige
Goal-Vorlagen auswählen. Das erweitert [dynamische Pläne](dynamic-plans.md)
gemäß `ARCHITECTURE.md`, v3 §5.2 HET-03/HET-04/HET-09, §5.3 und §8.2 LNG-02:
Ein Schritt kann Daten beschaffen, einen Handlungskandidaten vorschlagen,
Verification und menschliche Freigabe abwarten, handeln und frische Belege
für eine Wirkungsbewertung erfassen.

Die Ausführung bleibt bei der vorhandenen [Zielschleife](goal-runtime.md).
Der Planer bekommt weder eigene Act-Rechte noch einen neuen Freigabeport.
Der Plan-Snapshot bleibt ein `arcs.plan.v1` mit niedrigem Trust.

## Beispiel mit simuliertem Gerät

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/goal-plans/host.json \
  --runtime apps/adapter-host/config/goal-plans/runtime.json \
  --ticks 20
```

Journale liegen getrennt unter `var/goal-plans/`. Der externe Rules-Planer
`goal.workflow` wählt eine freigegebene Goal-Vorlage für den Auftrag. Ein
weiterer externer Rules-Reasoner schlägt die konkrete Ventilatoraktion vor.
Der Plan enthält den Kindauftrag `p1.device`; dieser erreicht
`awaiting_approval`. Bis zu seiner Freigabe gibt es keine Geräteaktion.
Die Demo verwendet deterministische Regeln und einen simulierten Ventilator.
Sie ist kein Nachweis für echte LLM-Planung oder reale Drohnensteuerung.

Den Kandidaten im Bericht unter `goals` prüfen. Anschließend eine Datei
`review-goal.json` mit der aktuellen Kindauftrags-ID und der exakten
Kandidatenversion erstellen:

```json
{
  "command": "review",
  "sequence": 2,
  "candidate_version_id": "<candidate aus dem aktuellen Goal-Bericht>",
  "decision": "approved",
  "operator_id": "operator.simon",
  "reason": "Konkrete simulierte Ventilatoraktion geprüft"
}
```

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/goal-plans/host.json \
  --runtime apps/adapter-host/config/goal-plans/runtime.json \
  --goal-command review-goal.json
./target/debug/arcs-host \
  --config apps/adapter-host/config/goal-plans/host.json \
  --runtime apps/adapter-host/config/goal-plans/runtime.json \
  --ticks 20
```

Der erste Aufruf speichert nur die Freigabe. Der anschließende Lauf führt die
simulierte Aktion aus, beschafft einen neuen Gerätezustand und lässt den
Goal-Assessor das Ergebnis bewerten. Danach ist das Goal `completed` und der
Plan `ready_for_review`. Ein weiterer Start wiederholt die Aktion nicht.
Bei bereits benutzten Journalen die tatsächlichen IDs aus dem Bericht verwenden.

## Vertrag und Grenzen

Eine Goal-Vorlage enthält die normale `ReasoningRoute` mit `goal` und einer
fest konfigurierten Aktionsmenge. Sie muss `goal.approval.mode: manual`
verwenden. Die bestehenden Verträge prüfen weiterhin Quellen, Query-Schemas,
Reasoner-Grants, idempotente Aktionen, Policy-Schemas und Kontextbudgets.
Auch nicht gewählte Vorlagen werden vor dem Öffnen des Runtime-Journals geprüft.
Automatische Policy-Freigaben bleiben in festen Goal-Routen verfügbar; in
dynamisch auswählbaren Goal-Vorlagen werden sie hier abgewiesen.

Der Root-Plan braucht `max_total_actions`, sobald sein Katalog Goals enthält:

```json
"plan": {
  "max_steps": 1,
  "max_total_runtime_ms": 4000,
  "max_total_actions": 2,
  "templates": [
    {"route": {"id": "switch", "…": "vollständige manuelle Goal-Route"},
     "export_fields": ["goal_status", "effect"]}
  ]
}
```

Das ist ein Strukturausschnitt; die vollständige Konfiguration steht in
[`runtime.json`](../config/goal-plans/runtime.json). Der externe Vorschlag
enthält nur Vorlagen- und Schritt-IDs sowie Abhängigkeiten:

```json
{"steps":[{"id":"device","template":"switch"}]}
```

Der Planer kann keine Quellen, Query-Payloads, Aktionsargumente, Freigaben,
Budgets oder maximalen Runden überschreiben. Er sieht im Vorlagenkatalog
zusätzlich `kind: goal`, das Ergebnis-Schema und die Runden-, Aktions- und
Assessment-Grenzen. Private Constraints und Query-Payloads werden dabei
nicht in seinen Katalog kopiert.

## Konservative Gesamtbudgets

Bei jeder gewählten Goal-Instanz wird vor der Aufnahme reserviert:

- **Aktionen:** höchstens `max_rounds` Action-Dispatches.
- **Reasoning-Zeit:** `max_rounds × (Planungsbudget + Assessment-Budget)`.
  Mit `initial_assessment` zählt ein zweites Assessment-Budget pro Runde.
- **Modellausgabetokens:** dieselben Aufrufzahlen für alle modellgestützten
  Planungs- und Bewertungsaufrufe, falls `max_total_output_tokens` gesetzt ist.

Mehrfache Auswahl derselben Vorlage wird mehrfach gezählt. Fehlende belegbare
Modell-Ausgabegrenzen und Überschreitungen führen zur Ablehnung des Vorschlags,
bevor Kindaufträge entstehen. `max_total_actions` ist eine Obergrenze von
0–31.000; mit 0 kann keine Goal-Instanz mit möglichen Aktionen aufgenommen werden.
Eine frühe Zielerfüllung kann weniger Aufrufe benötigen, erhöht aber nicht das
Budget anderer Schritte. Der Root-Planer hat weiterhin sein eigenes Budget.

Diese Grenzen zählen geplante logische Aufrufe. Backend-Replay folgt den
bestehenden stabilen Invocation-IDs und benötigt weiterhin einen idempotenten
Adapter. Die Reasoning-Zeit ist keine Kalenderfrist und enthält keine allgemeinen
Geld-, Request-, Geräte- oder Wirkungsbudgets. Query-/Act-Ports behalten ihre
eigenen Grants und Deadlines.

## Ergebnisbindung und Wiederaufnahme

Ein reiner Reasoning-Schritt exportiert seine `solved`-Kandidaten. Ein Goal
exportiert erst seinen abschließenden `arcs.goal_assessment_candidate.v1`-
Beleg, nachdem sein Checkpoint `completed` erreicht hat. Handlungsplan,
Freigabe und Ausführungsquittung reichen allein nicht aus, um einen Join
freizugeben. Die `export_fields` beziehen sich bei Goals auf diese Bewertung.
Beispielsweise kann ein Nachfolger nur `goal_status` und `effect` erhalten.
Gespeicherte Versionsbezüge, Schema und Produzent werden dabei geprüft.

Ein Goal darf durch den vorhandenen anfänglichen Zielcheck auch ohne Aktion
abschließen, wenn bereits ausreichende aktuelle Belege vorliegen. Bei einer
unklaren Bewertung, abgelehnter Freigabe, fehlenden Belegen oder erschöpften
Runden blockiert es den Gesamtplan beziehungsweise erreicht seinen normalen
terminalen Fehlerzustand. Es gibt keine zusätzliche automatische Neuplanung.

Request-Publisher werden für jede Betreiber-Vorlage vorregistriert. Die
Instanz verwendet dieselbe eingefrorene Query-Konfiguration mit eigenem
Auftrags-/Rundenbezug. So entstehen durch einen Neustart keine neuen
Query- oder Action-Identitäten. Bei verlorenen Ausführungs-Checkpoints wird
derselbe bereits belegte Dispatch wiedergefunden. Der bestehende Unterschied
zwischen synchronem Fehler-Replay und isolierten parallelen Fehlern bleibt
erhalten; fehlgeschlagene Schritte werden nicht pauschal automatisch wiederholt.

Termin-/Ereignisaktivierung und `between_rounds` bleiben auch in erzeugten Goals
nutzbar. Nach einer Monatswartephase folgen frische Belege und eine neue
konkrete Freigabe. Bei voller Queue empfängt die Runtime weiterhin benötigte
Beobachtungen. Schlafende Schritte benötigen keinen aktiven Reasoning-Worker.
Die anfängliche Beobachtungsgrenze eines erzeugten Goals wird erst bei Beginn
seiner Datenerfassung gespeichert. Beobachtungen aus der vorangegangenen
Planungs- oder Terminwartephase ersetzen dadurch keine frischen Daten.

## Abbruch und Prüfung

Der [Planabbruch](dynamic-plans.md#einen-plan-gezielt-abbrechen) stoppt auch
Goal-Kinder in `awaiting_approval` und aktualisiert ihre Checkpoints atomar.
Spätere Freigaben können diese Checkpoints nicht wieder aktivieren. Bereits
fertige Goals und ihre Nachweise bleiben erhalten. Das bestehende
`cancel_goal` kann ein einzelnes Goal stoppen; danach blockiert dessen Plan.
Live-Reasoning wird über seine tatsächlichen Request-IDs dem jeweiligen
Auftrag zugeordnet. Der nächste Tick verwirft auch nach individuellem Goal-
Abbruch ausstehende Antworten und gibt rechtzeitig antwortende Sessions
wieder frei. Dabei werden nur die begrenzten aktiven Worker geprüft.

```bash
cargo test --locked --offline -p arcs-host \
  --test goal_plans --test dynamic_plans --test plan_cancellation \
  --test goal_runtime --test goal_waits --test task_dependencies
```

Die Prozess-Tests prüfen konkrete Freigaben, Verifikation, unklare Wirkung,
Ergebnisexport, Budgetablehnung, den CLI-Vertrag, Neustart und verlorene
Ausführungs-Checkpoints. Ein alter Beobachtungsbeleg aus der Planungsphase
ersetzt keine frischen Eingangsdaten des erzeugten Goals. Eine simulierte Monatswartephase läuft bei voller
Queue mit frischen Beobachtungen und erneuter Freigabe. Reale Drohnen,
automatische Freigaben in erzeugten Plänen, direkte Act-Knoten, freie
Werkzeugargumente, rekursive Pläne, allgemeine Ressourcenleases und eine
universelle Zielzerlegung bleiben weitere Arbeit.
