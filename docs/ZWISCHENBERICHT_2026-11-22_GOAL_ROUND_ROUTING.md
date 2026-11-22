# ARCS-Zwischenbericht · 22. November 2026

## Planerauswahl pro Zielrunde

Betreiber können Zielrouten mit `reasoner_selection.reselect_each_round` ausstatten.
Sobald eine Runde ihre Planung beginnt, wählt der bestehende Reason-Manager anhand
der zugelassenen Optionen, aktuellen Verfügbarkeit, Binäreingaben und Ressourcen
einen Planer. Die Auswahl gilt für diese Runde über Wiederholungen und Neustarts
hinweg. Standardmäßig gilt die Auswahl für die gesamte Aufgabe.

Der Auswahlbeleg bleibt im gemeinsamen Artifact-Netz mit der ursprünglichen
Eingabe, der Runtime-Definition und der Runde verknüpft. Der Host prüft vor
Freigabe und Ausführung, ob der Ursprungsrequest zum Rundenbeleg gehört. Recovery
prüft zusätzlich den ursprünglichen Request und die zulässige Ersatzwahl.
Zielcheck und Wirkungsbewertung behalten ihre eigenen Aufträge.

CLI und Monitor zeigen die Rundennummer mit dem aktuellen Auswahlbeleg. Eine
Runde, die noch auf die Planung wartet, hat erst nach der Auswahl einen Eintrag.
Frühere Belege bleiben über die gespeicherten Requests nachvollziehbar.

## Verifikation

Die gezielten Host-Tests meldeten 14 bestandene Tests zur Auswahl und 15 zur
Recovery. Der neue Mehr-Runden-Test lässt den ursprünglichen Planer ausfallen
und weist nach, dass die zweite Runde den verfügbaren Planer selbst bindet.
Er prüft nach Neustart den unveränderten Rundenbeleg und lehnt eine verspätete
Freigabe für den Vorschlag der ersten Runde ab. Der simulierte Aktor bestätigt
jeweils genau eine Wirkung pro freigegebener Runde.

Der vollständige Rust-Workspace-Testlauf meldete 723 bestandene und
5 als ignoriert markierte Tests (`cargo test --workspace --locked --offline`).
