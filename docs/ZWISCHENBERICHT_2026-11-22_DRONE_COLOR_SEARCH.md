# Zwischenbericht vom 22. November 2026

Die Farbsuche ist jetzt als Anwendung im eigenständigen Simulatorprojekt
startbar. Sie erzeugt die Betreiberkonfiguration für den vorhandenen
ARCS-Host. Externe Reasoner verwenden die vorhandene PPM-Pixelauswertung
für Planung und Wirkungsprüfung. Die Kamera liefert zu jeder freigegebenen
Drehung eine Aufnahme mit der bestätigten Revision. Der Wirkungsprüfer
vergleicht Pose und Revision mit der vorherigen Aufnahme und der
Ausführungsquittung. Die Fundbewertung referenziert das konkrete Bild.

## Nachweise

- Der anfangs außerhalb des Sichtfelds liegende Marker wurde nach zwei
  Drehungen sichtbar. Die drei gespeicherten Aufnahmen hatten die
  Revisionen 0, 1 und 2. Die Quittungen der zwei Drehungen blieben beim
  erneuten Start jeweils dieselben.
- Bei einer fehlenden Zielfarbe endete die Suche nach zwei Runden mit
  `budget_exhausted`. Bei einem von Anfang an sichtbaren Ziel wurde der
  Auftrag auf Basis der ersten Aufnahme abgeschlossen.
- 33 Rust-Tests im Simulator-Workspace und drei Such-Prozesstests bestanden.

Die Strategie ist eine Farbsuche mit Drehungen an einem festen Standort im
Simulator. Der Simulatorquellstand ist im ersten lokalen Commit `84edcb7`
gesichert. `drone-sim` hat bisher kein Git-Remote.

[Startbefehle und Konfiguration](../../drone-sim/apps/drone-demo/README.md#wiederholbare-farbsuche)
