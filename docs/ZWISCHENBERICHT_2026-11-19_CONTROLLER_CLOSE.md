# ARCS – Bestätigte Controller-Übergabe, 19. November 2026

Für gesteuerte Ressourcen schließt der Controller jetzt die gehaltene
Reservierungsrevision vor dem Freigabe-Commit. Der Lüftersimulator speichert
die Sperre dauerhaft. Er weist verspätete Aufträge der alten Revision ab,
selbst wenn der Nachfolger noch keinen Auftrag gesendet hat.
[Adapterprotokoll](../ADAPTER_PROTOCOL.md#reservierungsrevision-an-der-act-grenze)

Der Core erfasst jede beteiligte Act-Capability vor dem Dispatch und speichert
ihre korrelierte Bestätigung. Beim Freigabe-Commit prüft derselbe
SQLite-Schreibpfad den aktuellen Besitzer und die offenen Bestätigungen.
Die Gerätekommunikation findet zwischen den kurzen Schreibtransaktionen
statt. Für ältere gehaltene Sitzungen ergänzt die Laufzeit mögliche
Controller aus der eingefrorenen Goal-Route.
[Architekturspezifikation, PAR-04/PAR-08/PAR-10 und §7.3](ARCHITECTURE.md)

Eine Ablehnung lässt den Besitz erhalten. In der Laufzeitübersicht erscheint
die ausstehende Bestätigung. Nach einem Ausfall des Core-Commits nutzt die
Laufzeit bereits gespeicherte Controller-Bestätigungen beim nächsten Versuch.
Beim Aufwachen nach einer Rundenpause beginnt eine neue Sitzung erst nach
Abschluss der alten Übergabe.
[Laufzeitstatus und Beispiel](../apps/adapter-host/examples/exclusive-goals.md)

Die Tests prüfen verspätete Befehle nach Wiederöffnung des Simulators, eine
während der Bestätigung neu eingetragene Act-Capability sowie einen
absichtlich unterbrochenen Freigabe-Commit mit anschließendem Neustart.
Die Geräteprüfung lief am simulierten Lüfter. Insgesamt waren 372 ausgewählte
ARCS-Tests und 33 Tests im separaten Drohnensimulator erfolgreich.
Workspace-Build und Formatprüfung liefen durch. Clippy meldete die zwei
vorhandenen Warnungen im Core-Reasoning-Service. Die Logs liegen unter
`var/verification-controller-close/`.

Die Abnahme an realen Controllern, nebenläufige Übergabedialoge und die
Langzeitabnahme stehen aus.
