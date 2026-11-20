# ARCS – Nebenläufige Aktionsausführung, 20. November 2026

Der Goal-Scheduler kann Aufgaben weiterführen, während ein Aktor antwortet.
Der Host verwendet bis zu vier Act-Worker. Jeder laufende Aufruf
belegt seine Adaptersitzung exklusiv. Diese Zulassung gilt
auch für Abfragen, Beobachtungen und die bestätigte Controller-Übergabe.
[Architekturspezifikation, PAR-01/PAR-02/PAR-04/PAR-07/PAR-08/PAR-10](ARCHITECTURE.md)

Freigabe, Action-Version, Aufrufidentität und gegebenenfalls die exakte
Reservierungsrevision werden vor dem Senden geprüft und gespeichert. Der
Host validiert die Antwort und speichert den Ausführungsbeleg im Core.
Beobachtungen für die Wirkungsprüfung beginnen am Commit dieses Belegs.
Bei einem endgültigen Aufgabenabbruch bleibt ein verspäteter Beleg im
Artifact-Netz erhalten. Der Monitor zeigt ihn in der Reservierungsprüfung.
Die Aufgabe bleibt beendet und eine ungeklärte Reservierung bleibt gehalten.
[Goal-Runtime](../apps/adapter-host/examples/goal-runtime.md#persistenz-betrieb-und-grenzen)

Der Prozesstest wartet, bis der Lüftersimulator die Aktion gespeichert hat,
und hält dann dessen Antwort zurück. Währenddessen erreicht eine unabhängige
Aufgabe ihren Review-Zustand. Nach einem Host-Neustart verwendet der
Simulator dieselbe Invocation-ID. Er zählt eine Gerätewirkung, und der
Core speichert genau einen Ausführungsbeleg. Ein zweiter Prozesstest bricht
das Goal während des laufenden Aufrufs ab und prüft den späten Beleg und
die gehaltene Reservierung.

Der Workspace-Build und die Formatprüfung waren erfolgreich. Die Tests
des separaten Drohnensimulators liefen mit 33 bestandenen Fällen durch.
Clippy meldete zwei bereits vorhandene Warnungen im Reasoning-Service.
Die Prüflogs liegen unter `var/verification-act-workers/`.

Die Abnahme an realen Controllern und die Langzeitabnahme stehen aus.
