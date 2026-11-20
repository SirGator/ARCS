# ARCS – Nebenläufiger Ereigniseingang, 20. November 2026

Der Aufgaben-Scheduler startet Input- und Observe-Polls auf den vorhandenen
stdio-Transport-Threads. Bis zu acht Poll- und ACK-Dialoge können zugleich
laufen. Jeder beansprucht seine Adaptersitzung exklusiv. Zwischen dem Poll
und dem ACK validiert und speichert der Core die Nachricht idempotent.
Die Bestätigung setzt den gespeicherten Beleg voraus.
[Architekturspezifikation, PAR-01/PAR-02/PAR-08/PAR-10/PAR-11](ARCHITECTURE.md)

Bei voller Queue lässt die Runtime Input-Quellen aktiver Weckbedingungen
weiter zu. Sie prüft die Zulassung beim Eintreffen der Antwort erneut.
Andere Inputs können im dauerhaften Ausgangspuffer des Adapters auf ihren
nächsten Poll warten. Ein Transport- oder Nachrichtenfehler nimmt die
betroffene Sitzung aus dem Polling. Der Monitor zeigt die Störung je Adapter.
Core-Speicherfehler werden als Betriebsfehler gemeldet.
[Aufgabenlaufzeit](../apps/adapter-host/examples/task-runtime.md)

Die Prozesstests halten einen Sensor-Poll zurück, während eine unabhängige
Aufgabe die Prüfung erreicht. Nach einer simulierten Sensorstörung nimmt
der Monitor einen weiteren Auftrag an. Ein zweiter Test stoppt den Host
nach dem Core-Commit und vor dem ACK. Beim Neustart bestätigt ARCS die
erneut gelieferte Nachricht und verwendet den gespeicherten Aufgabenstand.
Ein Fristentest prüft den Fortschritt der Uhr während eines ausstehenden
Sensor-Polls und das Verhalten mit expliziter Simulationszeit.

Workspace-Build und Formatprüfung liefen durch. Clippy meldete zwei
bestehende Warnungen im Reasoning-Service. Die Prüflogs liegen unter
`var/verification-ingress/`.

Die Abnahme an realen Controllern und im monatelangen Dauerbetrieb steht aus.
