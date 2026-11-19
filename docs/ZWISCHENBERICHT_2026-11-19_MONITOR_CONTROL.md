# ARCS – Steuerung im laufenden Host, 19. November 2026

Der laufende Host kann Aufgaben und Prüfserien per lokalem Monitor pausieren,
fortsetzen oder im Rahmen der vorhandenen Befehle abbrechen. Planabbrüche und
Goal-Entscheidungen verwenden denselben Kanal. Das erweitert PAR-07,
PAR-08, PAR-11 sowie LNG-05, LNG-08 und LNG-10. [Architekturspezifikation](ARCHITECTURE.md)

`--monitor --control-stdin` aktiviert die Standardeingabe für den
Betreiber. Der Monitor gibt korrelierte Ergebnisse als `control_result`
aus. Die normalen Einzelbefehle und der Monitor teilen ihre typisierten
Befehle und führen dieselben Runtime-Methoden aus. Damit gelten dieselben
Revisions-, Berechtigungs- und Freigabeprüfungen. Der Host bleibt Besitzer
der Queue, der Modelle und der Adapter. [Anwendung und Nachrichtenformat](../apps/adapter-host/examples/task-runtime.md#steuerung-im-laufenden-host)

Ein einzelner Reader prüft die Zeilengrenze und puffert höchstens 16 Rahmen.
Vor einer Runde werden bis zu acht Befehle bearbeitet. Überlange Zeilen,
unvollständige Zeilen am Eingabeende, inkorrektes JSON und abgewiesene
Befehle erhalten begrenzte Fehlerbelege. Die Verarbeitung der übrigen
Aufgaben geht weiter. Die Stop-Datei beendet den Monitor auch bei offenem
Eingabekanal.

Der Prozessnachweis sendet Befehle an einen laufenden Host. Er prüft
Serienpause, Wiederholung desselben Belegs, Fortsetzung, Abbruch,
Einzelaufgabenpause, Befehlsrückstau und Wiederaufnahme nach Neustart. Im
Lüftersimulator bleibt die Zahl der Wirkungen bei einer Freigabe mit falscher
Candidate-Version null. Die gültige Freigabe führt zu genau einer Wirkung,
auch beim erneuten Freigabeversuch und nach Neustart. Die Tests verwenden
lokale Testfreigaben und echte Adapterprozesse.

76 Tests in sieben Host-Testsuiten sind erfolgreich. Dazu zählen die neuen
Monitor-Prozesstests und die bestehenden Prüfungen für den Statusmonitor,
Goal-Freigaben, Planabbrüche, Aufgabenpausen, Terminserien und
Reasoning-Belege. Der Workspace-Build für Binärprogramme und Beispiele sowie
Format- und Diff-Prüfung sind erfolgreich. Clippy meldet die beiden
vorhandenen Warnungen im Core-Reasoning-Service. Lokale Protokolle liegen
unter `var/verification-monitor-control/`.

Die CLI bietet den Kanal für ein lokales Terminal oder eine Anwendung, die
den Monitor startet. Das Desktop-Kontrollpanel verwendet diesen Schreibkanal
noch nicht. Die Bearbeitung erfolgt an Tickgrenzen. Langzeitabnahme und
Bedienung über weitere Anwendungen bleiben offen.
