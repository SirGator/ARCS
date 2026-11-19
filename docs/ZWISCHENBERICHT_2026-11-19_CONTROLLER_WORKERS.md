# ARCS – Nebenläufige Controller-Übergabe, 19. November 2026

Der laufende Host führt Controllerdialoge in bis zu vier Hintergrundworkern
aus. Pro beteiligtem Adapter bleibt eine Session belegt. Währenddessen
bearbeitet der Scheduler unabhängige Aufgaben. Die Worker übertragen nur
die vorbereiteten Anfragen. Der Host speichert bestätigte Revisionen im
Core und prüft den Freigabe-Commit wie bisher atomar.
[Architekturspezifikation, PAR-01/PAR-02/PAR-04/PAR-08/PAR-10/PAR-11](ARCHITECTURE.md)

Der Monitor bestätigt die Annahme einer Betreiberentscheidung mit
`release_pending: true`, wenn Controller noch antworten müssen.
Die Entscheidung bleibt mit den exakten Revisionen und Belegen im Journal.
Nach einem Neustart fährt die Runtime damit fort. Die Desktop-Anzeige
meldet die angeforderte Übergabe bis zum Freigabe-Commit.
[Monitorprotokoll](../apps/adapter-host/examples/task-runtime.md#steuerung-im-laufenden-host)

Im Prozesstest hält ein Dateisignal den Controllerdialog des simulierten
Lüfters an. Eine unabhängige Aufgabe erreicht unterdessen ihr Ergebnis.
Der Benutzerbefehl kann unverändert wiederholt werden. Eine widersprechende
Entscheidung wird abgewiesen. Der Host startet neu, während der Controller
noch wartet, und veröffentlicht nach dessen Antwort die ursprüngliche
Freigabe mit genau einer vorherigen simulierten Gerätewirkung.

356 ausgewählte Rust-Tests, 46 GTK-Tests und 33 Tests im separaten
Drohnensimulator waren erfolgreich. Workspace-Build und Formatprüfung
liefen durch. Clippy meldete die beiden vorhandenen Warnungen im
Core-Reasoning-Service. Die lokalen Logs liegen unter
`var/verification-controller-workers/`.

Die Abnahme an realen Controllern und die Langzeitabnahme stehen aus.
