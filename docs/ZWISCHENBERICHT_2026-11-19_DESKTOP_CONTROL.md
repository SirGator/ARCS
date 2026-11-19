# ARCS – Steuerung und Freigaben im Desktop, 19. November 2026

Unter **Steuerung** zeigt ARCS Control die Goals, Pläne und Prüfserien des
laufenden Hosts. Die sichtbaren Einträge lassen sich mit Begründung pausieren,
fortsetzen oder abbrechen. Eine manuelle Goal-Entscheidung beginnt mit der
Vorschau der konkreten Aktion. Die Änderungen unterstützen PAR-07, PAR-08,
LNG-05 und LNG-08. [Architekturspezifikation](ARCHITECTURE.md)

Der Controller schreibt im Hintergrund in die Pipe seines eigenen
Host-Prozesses. Die Anfrage bleibt bis zur passenden Rückmeldung ausstehend.
Wenn die Sitzung davor endet, zeigt das Panel den offenen Ausgang an. Das
Panel verwendet die vorhandenen Revisions- und Freigabeprüfungen des Hosts.
Ein älteres Host-Binary kann weiter im Statusmodus laufen. [Bedienung im Kontrollpanel](../apps/control-panel/README.md)

Der Host lädt die Vorschau erst auf Betreiberanfrage. Er überprüft dabei den
wartenden Goal-Zustand, die Candidate-Version und den positiven Prüfbeleg.
Die Vorschau enthält die genaue Fähigkeit und die vollständigen
Aktionsparameter bis zur Größenbegrenzung. Der Dialog sendet die Entscheidung
mit der angezeigten Candidate-Version an denselben Host. Ein veränderter
Vorschlag muss erneut geöffnet werden. Die Freigabe bleibt eine Entscheidung
des Betreibers. [Protokoll](../apps/adapter-host/examples/task-runtime.md#steuerung-im-laufenden-host)

Für die GTK-Tests läuft eine private Anzeige über Unix-Sockets. Die Tests
bedienen das Panel gegen den regulären Host und echte Adapterprozesse. Im
Lüftersimulator werden Vorschau, Schließen und ausdrückliche Freigabe
getrennt geprüft. Die Prüfserie wird ohne Host-Neustart pausiert und bleibt
beim anschließenden Neustart pausiert. Das Panel setzt sie fort und speichert
den Abbruch auf Revision 3.

44 Panel-Tests, darunter der Planabbruch mit drei pausierten Teilschritten,
und 47 ausgewählte Rust-Tests waren erfolgreich. Der Workspace-Build für
Programme und Beispiele sowie Format- und Diff-Prüfung waren erfolgreich.
Clippy meldete die beiden vorhandenen Warnungen im Core-Reasoning-Service.
Lokale Logs stehen unter `var/verification-desktop-control/`. Die gerenderten
Ansichten der pausierten Serie und des Freigabedialogs liegen unter
`var/control-panel/screenshots/`.

Die Ansicht arbeitet mit den begrenzten Snapshot-Listen des Hosts. Eine
allgemeine Queue-Ansicht, die Klärung offener Aktoraufträge im Panel und die
Abnahme im Betrieb über Monate stehen aus.
