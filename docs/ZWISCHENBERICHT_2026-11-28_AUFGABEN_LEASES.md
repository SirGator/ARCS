# ARCS – Aufgaben-Leases und das Buch der externen Wirkungen, 28. November 2026

Dieser Bericht dokumentiert den sechsten umgesetzten Slice der Erweiterung
[Zielbilderweiterung v3.1](ZIELBILD_ERWEITERUNG_2026-11-27.md) aus dem Bereich
Dauerhafter Betrieb (`OPS-03`, `OPS-05` als Grundlage, `TEST-10`).

Der Slice hat keine neue Instanz, keinen siebten Manager und keine
zusätzliche Kontrollhierarchie eingeführt. Er erweitert die vorhandene
Task-Runtime, das Artifact-Netz und den Adaptervertrag um genau die zwei
Konzepte, die dort fehlten: **wem gehört eine Aufgabe** und **was ist mit
einer bereits ausgegangen externen Wirkung geschehen**.

## Ausgangslage

Die Analyse zeigte zwei Lücken, die beide in der Wiederaufnahme zusammen
wirkten:

1. **Keine Task-Ownership.** Es existierten exklusive Ressourcenreservationen
   mit Revisionen und Fences, aber diese galten einer *Ressource*
   (`device.fan-01`), nicht einer *Aufgabe*. Zwei Instanzen konnten dieselbe
   Aufgabe bearbeiten, weil nichts sagte, wem sie gehörte. Die vorhandene
   `invocation_id` verhindert nur die doppelte Zustellung derselben
   Nachricht, nicht die doppelte Bearbeitung eines Ziels.
2. **Kein Buch über externe Wirkungen.** Die Wirkung eines Act-Aufrufs war an
   die Invocation gebunden, aber die Invocation unterschied nur
   `succeeded`/`failed`. Ein Host-Ausfall mitten im externen Aufruf hinterließ
   einen `dispatched`-Zustand ohne Aussage darüber, ob die Wirkung bereits
   eingetreten war. Ein Neustart Wiederholte den Aufruf blind; eine neue
   Planungsrunde hätte eine **neue** Action erzeugt und damit eine **neue**
   Operation – also eine zweite Wirkung.

Beide Lücken sind in `TEST-10` beschrieben: „Node-/Host-Ausfall und
verspätete Resultate führen weder zur falschen Task-Zuordnung noch zur
unkontrollierten Wiederholung einer externen Wirkung. Unklare Wirkung wird
explizit geklärt.“

## Was geändert wurde

### Core: das Buch der externen Wirkungen

Neu `crates/arcs-core/src/runtime/effects.rs` mit dem Schema
`arcs.effect.v1`. Eine externe Wirkung ist jede Handlung, die außerhalb der
Instanz Spuren hinterlässt. Der Eintrag bindet die Wirkung dauerhaft an

- die **Operation** (`operation_id`, aus der `invocation_id` abgeleitet),
- die **Aufgabe** (`task_key`),
- die **Invocation** (`invocation_id`),
- den **Zustand** und die **Anzahl der Versuche**.

Zustände und ihre Bedeutung:

| Zustand | Bedeutung | Nächste Sendung |
| --- | --- | --- |
| `planned` | geplant, nicht ausgegangen | `Send` |
| `dispatched` | ausgegangen, Ausgang unbekannt | `Replay` (identische idempotente Sendung) |
| `confirmed` | eingetreten und belegt | `RecoverEvidence` (nur den Beleg holen) |
| `unclear` | Ausgang lässt sich der Wirkung nicht zuordnen | Klärung erforderlich |
| `failed` | hat nicht stattgefunden | `Send` |

Die Entscheidungen werden als `DispatchDecision` zurückgegeben, damit die
Aufrufgrenze die Zustände nicht selbst nachbildet.

### Core: Aufgaben-Leases mit Fencing-Token

Neu `crates/arcs-core/src/runtime/ownership.rs` mit dem Schema
`arcs.task_lease.v1`. Eine Lease ist die persistente Zugehörigkeit einer
Aufgabe zu genau einem Eigentümer. Jede neue Zuteilung erhöht das
Fencing-Token der Aufgabe; ein zurückgebliebener Eigentümer kann nach der
Übergabe nicht mehr schreiben. Die Lease lebt im Core und nicht im Journal,
damit sie Neustart und Knotenwechsel überlebt.

Der Eigentümer ist die **logische Runtime** (`config.runtime_id`), nicht der
Prozess: Ein Neustart derselben Runtime darf weiterarbeiten, eine andere
Runtime wird ausgelagert.

### Core: die Ausführungsgrenze

`crates/arcs-core/src/action/execution/service.rs` bindt beides zusammen. Der
Aufruf `execute`/`execute_reserved`/`prepare` verlangt jetzt einen
`task_key`; die Wirkung wird vor dem Aufruf gebucht und die Zuordnung zur
Aufgabe geprüft.

- Der Empfang einer **passenden** Referenz belegt: Die Wirkung ist
  eingetreten. Ein fehlgeschlagener Beleg (Veröffentlichung im Store) darf
  deshalb **nicht** zu einer zweiten Sendung führen; die Wiederholung holt nur
  den Beleg.
- Ein **nicht erreichbarer** oder **ablehnender** Adapter belegt: Es ist nichts
  passiert, ein neuer Versuch ist zulässig.
- Ein **Timeout** oder ein **Protokollfehler** belegt nichts. Die Sendung
  bleibt `dispatched` und darf mit derselben idempotenten Sendung wiederholt
  werden; die Wiederholung wird im Buch sichtbar gezählt.
- Eine **nicht zuordenbare** Antwort (falsche `invocation_id`, fehlender
  Beleg) belegt nichts und lässt sich keiner Wirkung zuordnen: Die Wirkung
  wird `unclear` und die Aufgabe geparkt.

### Host: parks the task, the operator clarifies

`apps/adapter-host/src/task_runtime/`:

- `ownership.rs`: `hold_task_lease` und `check_task_lease` sichern die
  exklusive Bearbeitung einer Aufgabe. Eine fremde Runtime führt die Aufgabe
  in einen Wartezustand, statt sie zu bearbeiten.
- `effects.rs`: `effect()` liest den Stand, `reconcile_effect()` klärt ihn
  ausdrücklich. Die Klärung trägt Betreiber und Begründung und ist damit in
  der Historie des Eintrags nachvollziehbar.
- `goal/gates.rs`: Eine Aufgabe mit ungeklärter Wirkung wird geparkt
  (`blocked`) und nicht erneut eingeplant.

### Host: das Operator-Kommando

Neu `EffectCommand` in `apps/adapter-host/src/operator_command.rs` mit den
Befehlen `inspect` und `reconcile`. Der Befehl ist über den Monitor-Kanal
(`{"kind":"effect", ...}`), über die CLI (`--effect-command`) und über die
API erreichbar. Der Empfang ist ein `CommandReceipt::Effect` mit Zustand,
Aufgabe, Beleg, Versuchszahl und Begründung.

## Nachweis

| Test | Wo | Was er zeigt |
| --- | --- | --- |
| `plans_an_effect_once_and_keeps_the_binding` | `arcs-core` | Eine Wirkung wird genau einmal angelegt und bleibt an ihre Bindung gebunden. |
| `confirms_a_dispatched_effect_with_its_evidence` | `arcs-core` | Der Beleg wird angenommen; eine bestätigte Wirkung wird nicht wiederholt. |
| `recovers_the_receipt_of_a_confirmed_effect_without_a_second_effect` | `arcs-core` | Der Beleg einer bestätigten Wirkung wird nachgeholt, ohne zweite Wirkung. |
| `refuses_a_second_dispatch_of_an_unresolved_effect` | `arcs-core` | Eine ungeklärte Wirkung verlangt die Klärung; erst danach wird der Weg frei. |
| `reconciles_an_occurred_effect_with_its_reference` | `arcs-core` | Die Klärung „eingetreten“ belegt die Wirkung mit ihrem Verweis. |
| `keeps_the_full_history_of_an_effect` | `arcs-core` | Jeder Zustandswechsel bleibt in der Historie sichtbar. |
| `an_unattributable_answer_stops_the_effect_and_demands_a_clarification` | `arcs-core` | Eine nicht zuordenbare Antwort erzeugt **keinen** zweiten Aufruf. |
| `a_clarified_effect_lets_the_same_task_continue_without_a_second_effect` | `arcs-core` | Nach der Klärung setzt dieselbe Aufgabe denselben Auftrag fort. |
| `an_effect_stays_bound_to_its_task_even_when_another_task_reuses_the_action` | `arcs-core` | Eine andere Aufgabe darf das Ergebnis einer fremden Wirkung nicht übernehmen. |
| `failed_execution_publication_leaves_the_same_dispatch_recoverable` | `arcs-core` | Ein verlorener Beleg wird nachgeholt, ohne die Wirkung zu wiederholen. |
| `a_host_failure_during_an_effect_keeps_the_effect_bound_to_its_task` | `arcs-host` (E2E) | Host-Ausfall mitten in der Wirkung: das Gerät wird **einmal** betätigt, die Wirkung bleibt an ihre Aufgabe gebunden (`planned → dispatched → dispatched → confirmed`, Versuchszahl 2). |
| `an_operator_inspects_an_unresolved_effect_before_clarifying_it` | `arcs-host` | Der Betreiber sieht den Stand, und die Klärung wird ohne Betreiber, ohne Begründung, ohne Beleg oder doppelt abgelehnt. |
| `a_clarified_effect_is_bound_to_its_task_and_keeps_its_history` | `arcs-host` | Die geklärte Wirkung behält ihre Aufgabe und ihre vollständige Historie. |

Damit ist `TEST-10` für die Task-Zuordnung, die ausbleibende doppelte Wirkung
und die ausdrückliche Klärung nachgewiesen. Offen bleiben die Teile von
`TEST-10`, die verteilte Execution Nodes betreffen (`OPS-02`).

## Erkenntnisse

1. **Die Wiederholung derselben idempotenten Sendung ist die *kontrollierte*
   Wiederholung.** Eine zu strenge Regel („eine abgesendete Wirkung wird nie
   erneut gesendet") brach die reguläre Wiederaufnahme nach einem Host-Ausfall
   und damit einen bestehenden, bewusst gewählten Vertrag: Der Adapter
   dedupliziert über die `invocation_id`. Die Regel wurde deshalb auf den
   tatsächlichen Risikofall verengt: Verboten ist die Wiederholung einer
   *anderen* Wirkung, nicht die Wiederholung *derselben* Sendung. Das Buch
   zählt die Wiederholung, damit sie sichtbar bleibt.

2. **Der Task-Bezug schützt stärker als die Wiederholungsregel.** Weil die
   Wirkung ihren `task_key` dauerhaft trägt, kann eine zweite Aufgabe das
   Ergebnis nicht als das eigene übernehmen, und eine neue Planungsrunde
   erzeugt eine neue Operation – also eine zweite Wirkung. Das ist die
   eigentlicheprotection gegen die unkontrollierte Wiederholung.

3. **Eine Klärung muss auch den Auftrag freigeben.** Nach der Klärung „nicht
   eingetreten“ ist die Wirkung `failed`, die zugehörige Invocation aber
   `failed` und damit nicht mehr ausführbar. Ohne die ausdrückliche Freigabe
   der Invocation (`InvocationService::release_failed`) könnte die Aufgabe
   nicht fortfahren. Die Freigabe ist eine Entscheidung, keine Automatik.

4. **Die Klärung darf keinen neuen Plan erzeugen.** Die erste Fassung setzte
   die geparkte Aufgabe auf eine neue Runde. Das wäre genau die unkontrollierte
   Wiederholung gewesen: eine neue Runde erzeugt eine neue Action, eine neue
   `invocation_id` und damit eine neue Wirkung. Die geparkte Aufgabe setzt
   stattdessen denselben Auftrag fort.

5. **Der Eigentümer ist die logische Instanz, nicht der Prozess.** Ein
   Neustart derselben Runtime muss weiterarbeiten dürfen; nur eine andere
   Runtime wird ausgelagert. Deshalb ist der Lease-Eigentümer die
   `runtime_id` und nicht eine Prozesskennung.

## Was offen bleibt

- `OPS-01` (Host-Lebenszyklus als eigener Dienst), `OPS-02` (verteilte
  Execution Nodes mit Identität, Kapazität und kurzlebigen Grants),
  `OPS-04` (externe Harnesses als begrenzte Capabilities) und `OPS-06`
  (Rollen- und Mitarbeiterprofile) sind nicht Teil dieses Slices.
- `OPS-05` ist nur in der Grundlage umgesetzt: Das Operations-Interface
  kennt Aufgaben, Wirkungen und Freigaben, aber noch nicht Knoten, Kosten und
  menschliche Übernahme in einem einheitlichen Bericht.
- `BIZ-01` bis `BIZ-06` und `TEST-11`, `TEST-12` bleiben offen.
- `TEST-10` ist für die Zuordnung und die Wiederholung nachgewiesen, nicht für
  den Ausfall eines entfernten Nodes (`OPS-02`).

## Bekannte Flakiness

Zwei der Integrationstests der Host-Suite (`formal_evidence`,
`task_deadlines`) sind unter Last des vollständigen Workspace-Laufs zeitlich
flaky: Sie rechnen mit simulierten Uhren und festen Wartezeiten. Beide
bestehen isoliert und als Datei. Der Effekt ist unabhängig von diesem Slice
und wurde nicht verändert.
