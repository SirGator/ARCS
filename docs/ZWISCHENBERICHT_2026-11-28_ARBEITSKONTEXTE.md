# ARCS – Arbeitskontexte und vereinte Multi-Context-Runtime, 28. November 2026

Dieser Bericht dokumentiert den zweiten umgesetzten Slice der Erweiterung
[Zielbilderweiterung v3.1](ZIELBILD_ERWEITERUNG_2026-11-27.md) aus dem Bereich
Unified Multi-Context Runtime (`CTX-01`, `CTX-03`, `CTX-05`, Grundlage von
`CTX-09`).

Umgesetzt wurde kein zweiter Core, kein zusätzlicher Manager und kein
Agent-Spawning für mehrere Aufgaben. Die bestehenden Verträge von Store,
Invocation, Journal und Adapter-Registry wurden erweitert.

## Ausgangslage

Die Analyse zeigte, dass die Task-Runtime ihren Zustand über das Journal
(`runtime_jobs`, `runtime_plans`, `runtime_job_dependencies`) und die
Ergebnisse über den Invocation-Store führte. Ein gebündeltes Arbeitskontext-Objekt
existierte nicht: Ziel, Task, Planrevision, Artefaktversionen, Authority-Scope,
Memory-Sicht, Priorität, Budget und laufende Operationen lagen verstreut in
mehreren Tabellen und Artefakten. Eine autorisierte Statusabfrage über eigene
Arbeitszustände war damit nicht möglich.

## Was geändert wurde

- Neu `crates/arcs-core/src/runtime/work_context.rs`: der Arbeitskontext ist ein
  Core-Artefakt mit dem Vertrag `arcs.work_context.v1`. Er referenziert Ziel,
  Task, Planrevision, Artefaktversionen, Arbeitszustand, Authority-Scope,
  zulässige Memory-Sicht, Priorität, Budget und die laufenden Operationen.
  Nutzdaten werden **nicht** kopiert; der Kontext nennt nur Versionen.
- Der Kontext ist versioniert und besitzt je Tätigkeit einen Current-State-Zeiger
  auf dem Subject `work-context:<runtime>:<task_sequence>`. Die Identität ist
  deterministisch aus Runtime und Task abgeleitet. Ein Neustart findet denselben
  Kontext wieder, statt einen zweiten anzulegen.
- `WorkContextService` sichert die Zustandsübergänge: `Pending → Running →
  Waiting/ReadyForReview/Failed → Completed/Cancelled`. Terminale Zustände
  bleiben terminal. Ein Wechsel ist an den gespeicherten Stand gebunden; ein
  veralteter Stand wird mit einem Versionskonflikt abgelehnt, statt eine fremde
  Fortschreibung zu überschreiben.
- Ein Kontext mit offenen Operationen kann nicht abgeschlossen werden. Eine
  Operation mit unklarem externem Ergebnis kennt den Zustand `unknown` und darf
  ausdrücklich reconciled werden, ohne die Wirkung zu wiederholen.
- Die Schutzstufe des Kontexts ist mindestens die stärkste Stufe der Artefakte,
  auf die er verweist. Ein Kontext, der auf vertrauliche Inhalte zeigt, ist
  selbst nicht weniger schutzbedürftig. Die Stufe kann mit der Zeit nur steigen.
- Neu `SqliteArtifactStore::append_current_resolved`: hängt eine Version an und
  setzt den Current-State im selben Commit, und liefert die gespeicherte Fassung
  mit aufgelöster Schutzstufe. Arbeitskontexte brauchen beide Wirkungen in
  einem Commit.
- Neu `apps/adapter-host/src/task_runtime/work_context.rs`: die Aufgabenruntime
  führt für jede bearbeitete Aufgabe einen Arbeitskontext. Der Entwurf wird aus
  der gespeicherten Route, dem Task-Input, dem Ziel und der Planrevision
  abgeleitet; Authority-Scope und Memory-Sicht beschreiben nur, was die
  Aufgabe darf. Die wirksame Berechtigung bleibt beim Grant des Adapters.
- Das Ergebnis einer Aufgabe wird als Operation gebunden. Der Statusbericht
  `TaskRuntime::work_contexts` nennt Zustand und Bindungen ohne Nutzdaten und
  wird im Monitor-Snapshot veröffentlicht.
- Der Journal liefert `plan_revision` (einschließlich der Vererbung auf
  Planabkömmlinge) und `recent_task_sequences` für den begrenzten Statusbericht.

## Nachweise

- `crates/arcs-core/src/runtime/work_context/tests.rs`: 19 Tests. Anlegen und
  Current-State, Idempotenz des Anlegens, Ablehnung einer abweichenden Route
  für dieselbe Aufgabe, Fortschreibung von Zustand, Wartebedingungen,
  Operationen und Ergebnissen, Ablehnung unzulässiger Übergänge, Ablehnung
  eines Abschlusses mit offenen Operationen, Ablehnung einer Wartebedingung
  ohne Grund, Ablehnung unbekannter Artefakt- und Schemareferenzen, Ablehnung
  eines zu großen Budgets, Vererbung und Nichtabsenkung der Schutzstufe,
  Ablehnung eines veralteten Stands, Fortsetzung nach Neustart und Ablehnung
  eines fremden Artefakts mit demselben Vertrag, idempotentes erneutes Binden
  desselben Ergebnisses und Ablehnung eines abweichenden Ergebnisses an
  dieselbe Operation.
- `apps/adapter-host/tests/work_contexts.rs`: 4 Ende-zu-Ende-Tests. Ein Task
  bindet sein Ergebnis an einen persistenten Arbeitskontext; der Kontext
  referenziert Artefakte, ohne deren Nutzdaten zu kopieren; zwei Tasks erhalten
  getrennte Kontexte im selben Core; ein Neustart setzt den Kontext fort, statt
  ihn neu anzulegen.

Der vollständige Workspace-Testlauf und `cargo clippy --workspace --all-targets`
sind fehlerfrei; die verbleibenden Clippy-Hinweise bestanden bereits vorher.

## Erkenntnisse

- Ein Arbeitskontext gehört in den Core, nicht in das Journal. Das Journal
  führt den operativen Zustand einer Warteschlange; der Kontext ist ein
  referenzierbares Artefakt und überlebt damit auch einen Wechsel des
  Prozesses.
- Der Kontext darf keine Nutzdaten kopieren. Sonst entstünde ein zweiter,
  schwerer kontrollierbarer Datenbestand neben dem Artefaktgraphen.
- Die Bindung eines Ergebnisses an Kontext, Task, Planrevision und Operation
  gelingt über eine Operation im Kontext. Damit braucht es keinen globalen
  veränderlichen „aktuellen Kontext“, der in parallelen Ausführungen Authority
  oder Ergebnisse vertauschen könnte.
- Der Statusbericht liest den gespeicherten Stand, nicht einen nebenläufig
  geführten Zustand. Deshalb ist er nach einem Neustart genauso belastbar wie
  während des Betriebs.
- Ein veralteter Stand muss scheitern. Die Versionsfortschreibung des Stores
  liefert dafür bereits die passende Grenze; sie musste nur benutzt werden.
- Das erneute Binden desselben Ergebnisses an dieselbe Operation muss
  idempotent sein. Der erste Workspace-Lauf nach der Einführung zeigte, dass
  drei bestehende End-to-End-Tests genau daran scheiterten: ein Task kann
  zweimal abgeschlossen werden (etwa bei zwischengespeichertem Ergebnis oder
  unterbrochener Fortsetzung), und das zweite Binden war als unzulässiger
  Übergang `Succeeded → Succeeded` abgewiesen worden. Die Regel lautet nun:
  derselbe Zustand mit demselben Ergebnis ist erlaubt, ein abweichendes
  Ergebnis an dieselbe Operation wird abgelehnt. Damit ist die Wiederholung
  einer bereits gebuchten Wirkung erkennbar, statt sie stillschweigend zu
  überschreiben.

## Offen

- `CTX-02` (gemeinsam genutzte Infrastruktur ohne vermischte KV-Caches),
  `CTX-04` (Fairness, Backpressure, Budgets im Scheduler), `CTX-06`
  (explizites Spawning), `CTX-07` (Ressourcenmessung) und `CTX-11`
  (physisch getrennte Speicher) sind nicht umgesetzt.
- `CTX-08` und `CTX-10` (bereichsübergreifende Zusammenarbeit über
  Informationsfreigaben) sind nur in der Vorstufe umgesetzt: Der Kontext führt
  eine Memory-Sicht, aber es gibt noch keine Freigabe zwischen Bereichen.
- Der Kontext bindet bisher nur die Ergebnisversion der Aufgabe. Toolantworten,
  Ereignisse und Freigaben werden über den Invocation-Store und das Journal
  geführt; eine gemeinsame Bindung an den Kontext ist noch nicht umgesetzt.
- Eine Tätigkeit im Wartezustand braucht keine Modellinstanz; das ist im
  Host bereits der Fall, wird aber noch nicht über den Kontext geprüft.
