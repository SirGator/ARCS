# ARCS – Dauerhafter Aufgabenabbruch, 19. November 2026

Unter **Aufgaben** lässt sich ein offener Auftrag mit Begründung abbrechen.
Im Filter **Alle Aufgaben** bleibt sein Endzustand sichtbar. Der laufende
Monitor und die CLI nehmen denselben revisionsgebundenen Aufgabenbefehl an.
Die Änderung setzt den manuellen Abbruch aus PAR-07 um und trägt zu PAR-08,
PAR-10 und LNG-04 bei. [Architekturspezifikation](ARCHITECTURE.md)

Das vorhandene Journal speichert Abbruchstatus, Begründung, Revision und
Beleg in einer Transaktion. Eine identische Wiederholung liefert auch nach
einem Neustart den ursprünglichen Beleg. Erst nach dem Commit verwirft der
Host ausstehende Reasoning- und Request-Antworten dieser Aufgabe. Belegte
Kapazität wird frei, sobald die jeweilige Antwort oder Deadline eintrifft.
[Vertrag für den Aufgabenabbruch](../apps/adapter-host/examples/task-pauses.md#aufgaben-abbrechen)

Beim Abbruch eines Plan-Hauptauftrags beendet der vorhandene Planabbruch
auch seine offenen Schritte. Beim Abbruch eines einzelnen Schritts wertet
der Plan dessen Ergebnis gemäß seinen Abhängigkeiten und seiner
Neuplanungsregel aus. Bei Prüfserien endet der ausgewählte Durchlauf und
spätere Termine bleiben bestehen. Eine ungeklärte exklusive Reservierung
nach einem simulierten Aktorauftrag bleibt über den Neustart bestehen.
[Bedienung im Kontrollpanel](../apps/control-panel/README.md)

Die Tests prüfen den atomaren Rollback bei einem fehlgeschlagenen
Journaleintrag, verspätete Worker-Antworten, fortlaufende unabhängige Arbeit,
die Wiederholung eines Abbruchs, die ursprünglichen Termine einer Prüfserie
und den Unterschied zwischen einem einzelnen Planschritt und dem Planabbruch.
Der GTK-Test prüft den Wechsel vom offenen Auftrag zum sichtbaren Endzustand
im Verlauf. Der Aktortest verwendet den simulierten Lüfter.

57 ausgewählte Rust-Tests und 45 GTK-Tests auf einer privaten Anzeige
waren erfolgreich. Der Workspace-Build und die Formatprüfung sind
erfolgreich. Clippy meldete die zwei vorhandenen Warnungen im
Core-Reasoning-Service. Die lokalen Logs stehen unter
`var/verification-task-cancel/`.

Die Klärung offener Aktoraufträge im Panel und die Langzeitabnahme stehen aus.
