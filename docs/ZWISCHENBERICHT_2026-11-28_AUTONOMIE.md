# Zwischenbericht: kontrollierte Autonomie (BIZ-05)

> Stand: 28. November 2026 · Neunter Slice der Zielbilderweiterung
> Bezug: [Zielbilderweiterung §22, BIZ-05](ZIELBILD_ERWEITERUNG_2026-11-27.md),
> [Implementierungsplan EXT-BIZ](IMPLEMENTATION_PLAN.md)

## 1. Ausgangslage

Die Freigabekette im Core war binär: Ein Candidate brauchte eine
Verifikation und eine Freigabe, sonst wurde nichts ausgeführt. Die
vorhandenen Grenzen (`resource_policy.rs`, `AdapterGrant`-Limits) begrenzten
Ressourcen und Datenmengen, aber nicht *Autonomie*. Es gab keine ausdrückliche
Regel, nach der eine risikoarme, ausdrücklich erlaubte Routinehandlung ohne
menschliche Entscheidung laufen durfte, und keine Regel, die eine bedingte
Autonomie an Budget-, Preis- und Risikogrenzen band.

Ein Modell konnte daher keinen Rechtsgrund für seine eigene Handlung
erzeugen – aber der Betreiber konnte seine Handlung auch nicht
sachgerecht begrenzen. BIZ-05 verlangt beides.

## 2. Was gebaut wurde

### 2.1 Kernverträge

`crates/arcs-core/src/decision/autonomy/` enthält:

- `contracts.rs`: `RiskLevel` (low…critical), `Environment`
  (development/test/production), `AutonomyLevel` (routine/bounded/
  human_decision), `CapabilityRule`, `AutonomyBounds`, `HumanTrigger`,
  `NonDelegableRule`, `BudgetPolicy`, `AutonomyPolicy`, `ActionProfile`,
  `AutonomyReason` sowie die persistierten Sichten `AutonomyDecision` und
  `AutonomyBudget`.
- `service.rs`: `AutonomyService` mit `publish_policy`, `assess`,
  `authorize` und `budget`.

Drei neue Schemas wurden in die Registry aufgenommen:

| Schema | Inhalt |
| --- | --- |
| `arcs.autonomy_policy.v1` | Betreiberregel: Fähigkeitsregeln, Stufengrenzen, Auslöser, nicht delegierbare Regeln, Budget |
| `arcs.autonomy_decision.v1` | Aufgezeichnete Einordnung mit Stufe, Gründen, Risiko, Umgebung und Preis |
| `arcs.autonomy_budget.v1` | Kumulatives Kostenbudget mit Limit und gebuchten Kosten |

### 2.2 Entscheidungslogik

Die Einordnung ist eine reine Funktion `assess(policy, profile)` und damit
sowohl für die Freigabe als auch für Vorschau und Bericht identisch
verwendbar. Sie ist durchgängig konservativ:

- **Risiko** ist das Maximum aus Betreibergrundrisiko und Vorschlag. Ein
  Vorschlag kann sein eigenes Risiko nicht herabstufen.
- **Preis** ist das Maximum aus Vorschlagangabe und Betreibererwartung
  (`expected_cost`). Eine zu niedrige Preisangabe hebt die Erwartung des
  Betreibers nicht auf; die gebuchte Kostenmenge kann dadurch nicht
  unterschritten werden.
- **Umgebung** stammt aus der Betreiberregel. Eine im Test freigegebene
  Fähigkeit ist in der Produktion nicht freigegeben.
- **Währung** muss mit Fähigkeitsregel und Budget übereinstimmen. Eine
  abweichende Währung würde das Budgetbuch verfälschen.
- **Unbekannte Fähigkeit**, **unbekannter Preis**, **Auslöser** und
  **nicht delegierbare Regeln** führen immer zu `HumanDecision`.

Die Stufen folgen BIZ-05:

1. `Routine` – risikoarm, ausdrücklich erlaubt, innerhalb der Routinegrenzen.
2. `Bounded` – bedingt autonom innerhalb von Preis-, Budget- und
   Risikogrenzen.
3. `HumanDecision` – der sichere Standard für alles, was nicht ausdrücklich
   freigegeben wurde.

### 2.3 Kein zweiter Freigabepfad

Die autonome Freigabe entsteht über die **bestehende** Approval-Kette. Der
`ApprovalService` wurde um `decide_with_basis` erweitert: Er erzeugt
unverändert ein `arcs.approval.v1` mit einem System-Akteur und ergänzt
optional eine zusätzliche Grundlage als `based_on`-Relation. `ActionService`
und `ExecutionService` wurden **nicht** angepasst; der bestehende
Nachweis über den End-to-End-Test in
`crates/arcs-core/src/decision/autonomy/tests.rs`
(`the_existing_action_chain_accepts_an_autonomous_approval`) belegt, dass
eine autonome Freigabe unverändert zu einem ausführbaren Auftrag wird.

Damit bleibt die Wirkungskette einheitlich: Candidate → Verification →
Approval → Action → Execution. Autonomie ersetzt nur den Menschen in der
Freigabestufe, nicht die Verifikation.

### 2.4 Budget als Artefakt mit optimistischem Vergleich

Das Budget ist ein Core-Artefakt (`arcs.autonomy_budget.v1`) mit einer
Versionsfolge je Budget. Eine Reservierung schreibt die neue Version nur,
wenn der gelesene Stand noch der aktuelle ist
(`append_current_compared`). Gewinnt ein anderer Vorgang den Wettlauf, wird
**keine** Freigabe erteilt (`BudgetRace`). Das Budget ist damit gegen
Überschreitung durch parallele Aufgaben geschützt.

Bleibt die Freigabe aus, wird die Buchung kompensierend zurückgenommen. Das
ist der Recovery-Fall: Ein Fehler nach der Buchung darf das Budget nicht
dauerhaft belasten. Schlägt die Rücknahme selbst fehl, bleibt die Buchung
stehen – ein Budget darf nicht durch einen Fehler kleiner erscheinen.

Für bedingte Autonomie ist ein Budget zwingend. Ohne kumulative Grenze wäre
nur der Preis je Wirkung begrenzt, die Summe der Wirkungen aber nicht.

### 2.5 Dokumentation der Entscheidung

Jede Einordnung – auch eine ablehnende – wird als `arcs.autonomy_decision.v1`
festgehalten, mit Stufe, maschinenlesbaren Gründen, wirksamem Risiko,
Umgebung und Preis sowie einer Relation auf die Betreiberregel und den
Kandidaten. Eine Eskalation ist damit kein Schweigen, sondern ein belegter
Vorgang: Der Betreiber sieht, dass ARCS die Grenze erkannt hat und
bewusst nicht gehandelt hat.

Die Entscheidung ist über `AutonomyService::decision` und
`AutonomyService::decisions` wieder lesbar. Das ist die Grundlage für einen
autorisierten Statusbericht über laufende und blockierte Vorgänge.

## 3. Nachweis

19 Tests in `crates/arcs-core/src/decision/autonomy/tests.rs`:

**Einordnung (rein):**

| Test | Aussage |
| --- | --- |
| `routine_work_inside_the_operator_bounds_is_allowed` | Routinearbeit innerhalb der Grenzen läuft autonom. |
| `an_unknown_capability_is_never_autonomous` | Unbekannte Fähigkeit → `HumanDecision`. |
| `the_operator_risk_dominates_the_claim_of_the_proposal` | Das Betreibererisiko schlägt die Behauptung des Vorschlags. |
| `a_missing_price_without_operator_expectation_is_never_autonomous` | Ohne Preis und ohne Erwartung → `HumanDecision`. |
| `a_price_above_the_bound_is_never_autonomous` | Preis über der Grenze → `HumanDecision`. |
| `a_currency_mismatch_is_never_autonomous` | Falsche Währung → `HumanDecision`. |
| `production_needs_an_explicit_operator_permission` | Produktion braucht ausdrückliche Erlaubnis. |
| `a_human_trigger_overrides_the_bounds` | Ein Auslöser schlägt die Grenzen. |
| `a_non_delegable_rule_overrides_the_bounds` | Nicht delegierbare Regeln schlagen die Grenzen. |
| `the_proposal_cannot_understate_the_price_below_the_operator_expectation` | Preisunterschätzung wird neutralisiert. |
| `bounded_autonomy_needs_a_budget` | Bedingte Autonomie ist eine eigene Stufe. |
| `a_policy_without_any_environment_grants_nothing` | Eine Regel ohne Umgebung erteilt nichts. |

**Wirkung auf die Kette:**

| Test | Aussage |
| --- | --- |
| `an_authorized_routine_action_yields_a_system_approval` | Autonome Freigabe entsteht als System-Approval mit den Relationen `approves` (Kandidat) und `based_on` (Prüfbericht, Autonomieentscheidung); das Budget wird gebucht. |
| `the_existing_action_chain_accepts_an_autonomous_approval` | Die bestehende Action-Kette akzeptiert die autonome Freigabe unverändert. |
| `an_escalated_action_leaves_no_approval_but_records_the_decision` | Eskalation erzeugt keine Freigabe, keine Kostenbuchung – aber eine Entscheidung. |
| `the_budget_stops_the_second_expensive_action` | Die zweite Wirkung über dem Budget wird eskaliert; das Budget bleibt unverändert beim ersten Stand. |
| `bounded_autonomy_without_a_budget_is_escalated` | Bedingte Autonomie ohne Budget wird eskaliert. |
| `a_failed_approval_releases_the_reserved_budget` | Recovery: Bleibt die Freigabe aus, wird die Buchung zurückgenommen. |
| `a_candidate_for_another_capability_is_rejected` | Ein Kandidat, der eine andere Fähigkeit verlangt, wird abgewiesen. |

## 4. Bewusste Grenzen

- **Autonomie ersetzt keine menschliche Verantwortung.** ARCS ist keine
  Rechtsperson. `HumanDecision` ist der Standard, und die
  nicht delegierbaren Regeln bilden gesetzliche, vertragliche und
  betreiberpolitische Grenzen ab, die der Betreiber einträgt. Der Core
  ersetzt diese Grenze nicht, er Durchsetzung.
- **Verifikation bleibt Pflicht.** Autonomie betrifft nur die
  Freigabestufe. Eine autonome Freigabe setzt weiterhin einen
  Prüfbericht voraus, der den Kandidaten verifiziert.
- **Keine Wirkung ohne Freigabe.** Der Dienst erzeugt nur eine Freigabe; die
  Ausführung bleibt bei `ExecutionService` und dessen
  Eingriffsgrenzen.
- **Freigabe gilt nur für die geprüfte Operation.** Die Freigabe bezieht
  sich über die Relationskette auf genau einen Kandidaten.
- **Budgetbuchung ist konservativ.** Eine gebuchte Kostenmenge wird nicht
  erst bei der Ausführung verbraucht, sondern bei der Freigabe. Führt die
  Freigabe nie zur Ausführung, bleibt der Betrag belastet. Das ist
  gegenüber dem Betreiber sicherer als eine spätere Rückrechnung.

## 5. Offene Punkte

- `BIZ-01` (Organisationsziele, Rollen, Budgets, Regeln als Artefakte):
  Die Betreiberregel ist bereits ein Artefakt und damit ein erster Baustein.
  Ziele, Rollenprofile, Stakeholder und Fristen fehlen.
- `BIZ-02` (Requirements-Reasoning mit Annahmen, Hypothesen und Rückfragen)
  und `BIZ-04` (ereignisbasierte Prozesskette) sind nicht begonnen.
- `BIZ-06` (Entscheidungsabsicherung, Erfolgsmessung an tatsächlichen
  Ergebnissen) ist nicht begonnen.
- `TEST-11` ist nur in seinen Bausteinen belegt (Budgetüberschreitung und
  unautorisierte Wirkung werden blockiert). Der vollständige betriebliche
  Prozess – unvollständiger Kundenauftrag über belegte Anforderungen,
  Hypothesen, Rückfragen, Plan, Umsetzung und überprüfte Ergebnisse – ist
  noch nicht nachgewiesen.
- Die Autonomiestufen sind bisher an eine einzelne Aktion gebunden. Eine
  Fortschreibung der Regel während laufender Aufgaben ist nicht geregelt;
  eine Freigabe bezieht sich immer auf den Stand der Policy-Version, die
  in der Anfrage genannt wurde.
