# ARCS – Aufgabenwarteschlange im Desktop, 19. November 2026

Unter **Aufgaben** zeigt ARCS Control jetzt auch gewöhnliche Reasoning-Aufträge
und Teilschritte. Die Seite liest offene Aufgaben oder den gesamten Verlauf
der ausgewählten Runtime. Sie zeigt Wartegründe, Priorität, Frist und
zugehörige Pläne oder Serien. Eine eigene Pause lässt sich mit Begründung und
der angezeigten Revision setzen oder aufheben. Die Änderung unterstützt
PAR-01, PAR-07, PAR-11 und LNG-05. [Architekturspezifikation](ARCHITECTURE.md)

Der Host liest die Daten gezielt auf Anfrage aus dem vorhandenen Journal.
Die erste Seite bestimmt die höchste Aufgabensequenz für den Durchgang.
Weitere Seiten lesen ab der letzten Sequenz bis zu dieser Grenze. Neu
angelegte Aufträge erscheinen beim nächsten Aktualisieren. Der Zustand
bereits angelegter Aufträge wird bei jeder Abfrage neu gelesen. Für offene
Aufgaben verwendet der Host den vorhandenen Index nach Zustand und Sequenz.
[Protokoll der Aufgabenseiten](../apps/adapter-host/examples/task-runtime.md#aufgabenwarteschlange-lesen)

Der Monitor überträgt pro Seite höchstens 100 Aufgaben mit Betriebszustand
und Versionsreferenzen. Das Panel fragt 25 Zeilen ab und verwendet für
Änderungen die vorhandenen Betreiberbefehle. Dabei unterscheidet es eine
eigene Pause von einer Pause durch Plan oder Serie.
[Bedienung im Kontrollpanel](../apps/control-panel/README.md)

Der Prozesstest liest eine erste Seite mit Termin, nimmt währenddessen eine
weitere Eingabe auf und prüft die Folgeseite innerhalb der alten Grenze.
Er prüft Ereignis- und Abhängigkeitswartezeiten, die Historie und abgewiesene
Seitengrenzen. Der GTK-Test blättert durch 26 terminierte Aufträge und
pausiert und setzt den letzten Auftrag im selben Host fort. Die Erweiterung
des Plantests liest die von einem pausierten Plan übernommenen Pausen seiner
Teilschritte aus derselben Ansicht.

45 Panel-Tests auf einer privaten GTK-Anzeige und 57 ausgewählte Rust-Tests
waren erfolgreich. Der Workspace-Build, Format- und Diff-Prüfung liefen
erfolgreich durch. Clippy meldete die beiden vorhandenen Warnungen im
Core-Reasoning-Service. Lokale Logs stehen unter
`var/verification-task-queue/`. Die gerenderte zweite Aufgabenseite liegt unter
`var/control-panel/screenshots/task-queue.png`.

Die Queue steuert einzelne Aufgaben über Pause und Fortsetzung. Die
Fachansicht **Steuerung** bietet Abbrüche für Goals, Pläne und Serien. Die
Klärung offener Aktoraufträge im Panel und die Langzeitabnahme stehen aus.
