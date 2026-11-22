# ARCS-Zwischenbericht · 22. November 2026

## Schema-Verträge für Plan-Schritte

Eine empfangende Planvorlage kann mit `accepted_dependency_schemas` festlegen,
welche Ergebnisschemas sie über Abhängigkeiten annimmt. Auch bei passendem
Feldnamen weist ARCS eine Kante mit unpassendem Schema ab, bevor es die
Schritte des Vorschlags anlegt. Der Planerkatalog zeigt die tatsächlichen
Exportverträge. Das umfasst Reasoner-Ergebnisse, Goal-Wirkungsbewertungen,
Ausführungsquittungen und konfigurierte Fehlerquittungen.

Der Host verwendet dieselbe Schema-Ermittlung bei Planprüfung und
Ergebnisbindung. Vorgängerbelege aus einer früheren Revision behalten ihre
freigegebenen Felder und Artefaktversionen. Beim Laden eines angenommenen
Plans prüft der Host den Vorschlag erneut gegen die Betreiberkonfiguration.

Die Option ist pro empfangender Vorlage konfigurierbar. Eine leere Liste
erlaubt einen Schritt ohne Abhängigkeiten. Damit kann der Planer passende
Übergaben anhand eindeutiger Verträge bilden. Die automatische Suche nach
passenden Transform-Schritten bleibt offen.

## Verifikation

75 Host-Prozesstests bestanden. Geprüft wurden unter anderem ein kompatibler
Join, die Ablehnung unpassender Schemas vor der Kindaufnahme, empfangene
Wirkungsbewertungen und Ausführungsquittungen sowie reguläre und fehlgeschlagene
Exporte früherer Revisionen. Zwei bereits als ignoriert markierte
Umgebungstests blieben übersprungen. Der Rust-Workspace bestand die Prüfung
aller Targets.
