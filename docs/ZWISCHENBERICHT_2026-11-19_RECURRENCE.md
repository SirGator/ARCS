# ARCS – Wiederkehrende Prüfungen, 19. November 2026

Der Scheduler kann eine Prüfung in festen Abständen für dieselbe ursprüngliche
Eingabe einplanen. Damit setzt die Runtime einen weiteren Teil von LNG-04
sowie PAR-06, PAR-10 und PAR-11 um. [Architekturspezifikation](ARCHITECTURE.md)

## Verhalten

Die optionale Routenpolicy `recurrence` bestimmt das Zeitraster und die Zahl
der vorgesehenen Termine. Der erste Termin liegt bei Aufnahme der Eingabe.
Eine Serie belegt zwischen Prüfungen keinen Platz in der Aufgabenqueue. Pro
Serie darf höchstens eine Prüfung offen sein. Neue Eingaben und Folgetermine
wechseln sich bei der bevorzugten Aufnahme in die Queue ab. Die Runtime
speichert auch diesen Wechsel im Journal.

Nach einer Unterbrechung nimmt ARCS den jüngsten fälligen Termin auf, sobald
die vorherige Prüfung abgeschlossen ist oder einen terminalen Zustand erreicht
hat und ein Queueplatz frei ist. Ältere ausgefallene Termine zählen als
übersprungen. Die ursprüngliche Taktung bleibt erhalten. Ein abgelaufenes
Raster kann beim Wiederanlauf eine letzte Prüfung auslösen. Danach sind alle
Positionen im Raster verarbeitet.

Die Policy gilt für eigenständige Reasoning-Routen ohne Aktionsrechte. Jede
Prüfung besitzt einen eigenen Auftrag mit eigenem Budget. Die Eingabeversion bleibt für
alle Termine dieselbe. Neue Informationen können über den bereits
konfigurierbaren Gedächtnisabruf hinzukommen. [Konfiguration und Berichte](../apps/adapter-host/examples/scheduled-runtime.md#wiederkehrende-prüfungen)

## Wiederaufnahme

Der Discovery-Cursor, die Serie und der erste Auftrag werden gemeinsam
committet. Bei einem Folgetermin werden Terminbeleg, Auftrag und die nächste
Fälligkeit in einer Transaktion geschrieben. Die Auftrags-ID bindet spätere
Reasoning-Aufrufe an genau diesen Termin. Der Core speichert die dafür
verwendeten Termindaten im Reasoning-Request. Nach einem Fehler zwischen
Core-Ergebnis und Queue-Abschluss kann der Auftrag das bestehende Ergebnis
wiederverwenden.

Die gemeinsame Routenauflösung verbindet statische Routen, Prüfserien und
Planaufträge mit ihren jeweiligen Herkunftsbelegen. Die Berechnung der Fälligkeit
liegt im Scheduler. Die bestehenden Manager verarbeiten die Aufträge.

## Prüfungen

Vier zusätzliche Fälle in `scheduled_runtime` prüfen den Neustart nach
simulierten Monaten, verpasste Termine, stabile Reasoning-Identitäten,
Transaktionsfehler, den Wechsel bei einem einzigen Queueplatz und ungültige
Konfigurationen. Die Fälle starten die regulären Adapterprozesse und lesen
die gespeicherten Belege.

Alle 638 Workspace-Tests sind erfolgreich, darunter 13 Scheduler-Tests.
Der Build für alle Binärprogramme und Beispiele sowie die Format- und
Diff-Prüfungen sind erfolgreich. Clippy meldet die beiden vorhandenen
Warnungen in `reasoning/service.rs`.

Ein separater Lauf des regulären `arcs-host` mit den Rust-Adaptern und
Systemzeit prüfte vier Rasterpositionen im Abstand von sechs Sekunden. Die
CLI nahm die Termine 0, 2 und 3 auf. Termin 1 wurde als übersprungen gezählt.
Ein weiterer Start nach Serienende erzeugte keinen zusätzlichen
Reasoning-Aufruf. Die Queue war zwischen Prüfungen leer. Lokale Protokolle
und der Audit liegen unter `var/verification-recurrence/`.

## Offene Arbeit

Kalenderregeln und die Steuerung einer ganzen Serie bleiben offen. Fehler
folgen dem vorhandenen Betriebsmodus der Runtime. Im vollständig synchronen
Modus wird ein fehlgeschlagener Auftrag beim nächsten Lauf erneut versucht.
Paralleler Betrieb isoliert fehlgeschlagene Aufträge. Die Langzeitabnahme
im laufenden Betrieb steht aus.
