# Mathematischer Reasoner und Modell-Testadapter

`arcs-example-reasoner <profile.json> <journal.sqlite>` bietet
`solution.propose` als Reason-Capability über das gemeinsame SDK an.
Ein Profil nennt Adapter-ID und `method`:

- `solver`: Ein endlicher Optimierer sucht einen Wert mit minimalem
  absoluten Abstand zum Ziel. Bei Gleichstand bleibt der erste Wert erhalten.
- `learned`: Ein ausdrücklich gekennzeichneter Testadapter liefert den
  ersten Wert. Er führt keine echte Modellinferenz aus.

Beide Varianten verwenden dieselben V2-Nachrichten und Task-/Candidate-
Schemas. Der Solver benötigt ein Iterationsbudget und keine Tokens, Prompts
oder Temperaturen. Leere Wertelisten ergeben `no_solution`; ein zu kleines
Iterationsbudget ergibt `budget_exhausted` ohne Kandidaten.

SQLite bindet das Profil und speichert vollständige Invocations samt Antwort.
Eine verlorene Antwort lässt sich nach Neustart abrufen; geänderte Daten
unter derselben Invocation-ID werden abgewiesen. Der Algorithmus akzeptiert
höchstens 4096 endliche Werte. Die Datei-/Journalverzeichnisse müssen existieren.

Die Pakete hängen nur von Protokoll, SDK, Serde/JSON und SQLite ab.
Verifikation, Grants und Freigaben bleiben beim Host/Core. Ein korrekt
geformter Candidate ist kein Beweis einer optimalen oder richtigen Lösung.
Ein echter LLM-/ML-/NERVA-Adapter kann den gleichen SDK-Callback implementieren.

Konfiguration, Grants und Prozessnachweis stehen in der
[Input-/Reasoning-Anleitung](../../apps/adapter-host/examples/reason-task.md).
