# Erstes großes Anwendungsziel: Drohnensuche im simulierten Raum

## Festgelegtes Ziel

ARCS soll eine simulierte Drohne durch einen anfangs unbekannten simulierten
Raum ohne vorgegebene Karte steuern und dabei mit der Drohnenkamera ein
vorgegebenes Zielobjekt finden.

ARCS stellt als Harness verlässliche Sensordaten und nachvollziehbaren Kontext
bereit; angeschlossene Reasoner übernehmen Analyse, Planung und Vorschläge.
Die erste Priorität ist eine durchgängige Datengrundlage mit klaren Verträgen,
zeitlicher Zuordnung, Herkunft und erkennbaren Unsicherheiten.

Der Simulator kennt Raum und Zielposition für Physik und Bilderzeugung.
ARCS erhält die vorgesehene Sensorik und baut daraus sein Umgebungswissen auf.
Die vollständige Raumkarte und versteckte Zielkoordinaten dürfen ihm die
Erkundung und kamerabasierte Erkennung nicht vorwegnehmen. Die Suche dient
als erster Nachweis des allgemeinen Zusammenspiels von Datenbereitstellung,
Reasoning, kontrolliertem Handeln und Wirkungsprüfung.

Die bestehende Trennung gilt weiter: Der Core enthält allgemeine
Artefakt-, World-, Reasoning-, Verification- und Action-Logik. Konkrete
Geräteadapter, Kamera, Simulationscode und fachliche Demo liegen im
eigenständigen Projekt `../drone-sim`, mit eigenem Repository und Cargo-
Workspace. ARCS-Host und Core stellen den allgemeinen Harness und die
Anschlussverträge bereit.

## Verbindliche Architekturvorgabe: Universalität

Die Drohnensuche ist der erste große Anwendungsfall des allgemeinen
ARCS-Frameworks. Jede Erweiterung muss diese Wiederverwendbarkeit erhalten.

- **Allgemeiner Core:** Zielstatus, Evidenz, Abbruchbedingungen und der
  Ablauf Beobachtung → Entscheidung → Verification → Approval → Action →
  Wirkungsprüfung werden geräteunabhängig modelliert, soweit sie für den
  nächsten nachweisbaren Schritt erforderlich sind.
- **Austauschbare Fähigkeiten:** Anwendungen nutzen deklarierte
  Capabilities, Schemas und Grants über das Adapterprotokoll. Der Core
  erhält keine Fallunterscheidungen für bestimmte Drohnen oder Simulatoren.
- **Fachliche Verträge außerhalb des Core:** Pose, Flugbefehle,
  Kameramodelle und Objekterkennung gehören zu ihren jeweiligen Komponenten.
  Allgemeine Artefakte dürfen weiterhin unterschiedliche fachliche Daten
  tragen; ein universeller Core benötigt kein eingebautes Drohnenmodell.
- **Anwendung konfiguriert die Aufgabe:** Zielobjekt, Suchstrategie,
  Bewegungsgrenzen und Freigabepolicy liegen in der Anwendung beziehungsweise
  ihrer Konfiguration. Wiederverwendbarer Ablauf und fachliche Entscheidung
  bleiben getrennt.
- **Abstraktionen anhand konkreter Fälle:** Gemeinsame Mechanismen werden
  mit der Drohnensuche und den vorhandenen Sensor-/Lüfterfällen abgeglichen.
  Neue Core-Schnittstellen brauchen einen begründeten allgemeinen Nutzen.
  Spezialisierungen können zunächst außerhalb des Core entstehen.

Zusätzlich zur erfolgreichen Suche gilt als Abnahmekriterium: Der Core hat
keine Abhängigkeit von Drohnen-, Kamera- oder Simulationspaketen. Die
vorhandenen Sensor- und Lüfteranwendungen bleiben funktionsfähig. Geänderte
allgemeine Abläufe werden auch an einem fachlich anderen Fall geprüft.

## Ausgangslage

Vorhanden sind der allgemeine konfigurierbare Adapter-Host, separate
Sensor- und Lüfter-Simulatoren, persistierte Observations und Requests,
World-State-Replay sowie die kontrollierte Ausführung freigegebener Actions.
Für größere Inhalte gibt es außerdem allgemeine Datenreferenzen und einen
begrenzten, gesondert freigegebenen Abruf. Die Dateiquelle weist diesen Weg
mit persistenten Snapshots nach; sie erzeugt und analysiert keine Kamerabilder.
Ein allgemeiner Transform-Port verarbeitet inzwischen persistierte Eingaben
über externe Adapter und speichert abgeleitete Ergebnisse mit Provenienz.
Das Zahlen-Beispiel weist diese Grenze ohne Drohnen- oder Kamerawissen nach.
Ein allgemeiner Output-Port kann freigegebene Ergebnisse inzwischen an eine
Ausgabesenke liefern. Die Dateiausgabe demonstriert das mit gebundenem Auftrag,
separater Verification und Approval sowie einem dauerhaften Zustellungsbeleg.
Eine konfigurierte Beispielanwendung verbindet inzwischen Sensor, allgemeinen
JSON-Berichtstransform und Dateiausgabe über drei echte Adapterprozesse.
Sie weist Quellenauswahl, Provenienz, Trust-Grenze, explizite Exportpolicy
und Wiederaufnahme ohne zusätzliche Zustellungen nach.
Der allgemeine Host-Runner kann außerdem mehrere Transform-Stufen ausführen,
deren Topologie und Manifeste an eine Pipeline-ID binden und Fortschritt im
Store speichern. Ausstehende Beobachtungen werden hinter einem dauerhaften
Cursor verarbeitet. Freigabepolicies bleiben bei den Anwendungen.

Eine einfache Raum- und Drohnensimulation ist jetzt vorhanden. Sie bewegt
eine Drohne bei fester Höhe, prüft Wände und Hindernisse entlang der ganzen
Strecke und speichert Zustand, Bewegungsjournal und Telemetrie-Outbox.
`../drone-sim/apps/drone-demo` weist eine freigegebene Bewegung mit separater Pose-Abfrage,
persistierter Nachher-Observation und Outcome nach. Ein Neustart wiederholt
die Bewegung nicht. Der Core wurde dafür nicht geändert.

Die externe Kamera liefert inzwischen zugeordnete Bildsnapshots. Bildwahrnehmung,
ein Suchauftrag, eine fortlaufende Suchsteuerung und eine interaktive Anzeige
sind noch nicht vorhanden.
Die bestehende Host-CLI führt begrenzte Abfragen und Polling-Runden aus;
die vollständige Suchschleife fehlt noch. Ihr allgemeiner Ablauf soll
wiederverwendbar sein; die Anwendung ergänzt die Suchstrategie und Policy.

## Vorschlag für den ersten beherrschbaren Durchstich

- Ein einzelner Raum mit festen Wänden, wenigen Hindernissen und einem
  eindeutig beschriebenen Zielobjekt.
- Eine Drohne mit einfachen, begrenzten Bewegungsbefehlen und einer Kamera.
- Zunächst vereinfachte Flugbewegung, beispielsweise in fester Höhe.
  Realistische Flugphysik ist eine spätere Erweiterung.
- Eine einfache deterministische Suchstrategie als erster Ausgangspunkt.
- Ein nachvollziehbarer Fundbericht mit Referenz auf das Kamerabild.

Die Bewegung verwendet inzwischen eine eigene deterministische Rust-
Simulation bei fester Höhe. Die optionale Kamera verwendet inzwischen eine
Pinhole-Projektion und RGB-Bilder im PPM-P6-Format mit farbigen Wandmarkierungen.
Erkennungsverfahren und die vollständige Suchsteuerung sind noch offen.

## Geplante Zuständigkeiten

Bewegung und Kamera-Snapshots sind implementiert; Wahrnehmung und Suche folgen:

| Ort | Verantwortung und Stand |
| --- | --- |
| `../drone-sim/simulation/room/` | Raum, Pose, Bewegung, Kollisionen und perspektivische Kamerabilder implementiert |
| `../drone-sim/adapters/simulated-drone/` | Telemetrie, Pose-Abfrage, idempotente Bewegung und optionale Kamera-Snapshots mit Datenabruf implementiert |
| `../drone-sim/apps/drone-demo/` | Einzelner Bewegungsauftrag, Verification, System-Approval und Wirkungsprüfung implementiert |
| `../drone-sim/adapters/camera-perception/` | Geplant: Bildanalyse mit referenziertem Eingangsbild, Fundkandidaten und beobachteten Bildpositionen |
| `../drone-sim/apps/drone-search/` | Geplant: Suchauftrag, Strategie, fortlaufender Ablauf und Fundbericht |
| `crates/arcs-core/` | Bestehende allgemeine Verträge und Kontrollmechanismen; für die Bewegung unverändert |

Simulation und Drohnenadapter laufen derzeit in einem Prozess, mit der
Bewegungslogik in einer separaten Bibliothek. Beide gehören zum externen
Projekt und werden nicht in den ARCS-Host eingebaut. Auch die Kamerawahrnehmung kann zunächst
ein einfaches Verfahren verwenden, muss aber die erzeugten Bilddaten auswerten.
Objektspezifische Regeln und die Simulationsengine gehören nicht in den Core.

## Daten- und Kontrollfluss

```text
Suchauftrag
    ↓
Kamerabild + Drohnentelemetrie → gespeicherte Observation
    ↓
Bildauswertung → Fundkandidat mit Bildreferenz → World-State
    ↓
Suchentscheidung → Verification → Approval → Bewegungs-Action
    ↓
Drohnenadapter → Simulation → neue Telemetrie und Kamerabilder
    ↓
Fund prüfen oder weiter suchen; bei Abbruchbedingung stoppen
```

Eine erfolgreiche Befehlsannahme belegt noch keine erreichte Position.
Die Anwendung prüft die Wirkung anhand neuer Telemetrie und Bilder.
Ein Fundkandidat ist noch kein bestätigter Sucherfolg.

Der Agent darf für die Objektfindung keine versteckte Liste mit wahren
Zielpositionen erhalten. Die Simulation darf ihren vollständigen Zustand
für Darstellung und Tests kennen; dieser Zugang bleibt von den
Beobachtungen der Suchsteuerung getrennt. Telemetrie und weitere für die
Navigation verfügbare Informationen müssen ausdrücklich definiert werden.

## Etappen und Abnahme

1. **Raum und Drohne:** Ein reproduzierbares Szenario lässt sich starten.
   Begrenzte Bewegungen ändern die Pose, Wände und Hindernisse werden
   berücksichtigt, und der Zustand lässt sich nachvollziehen.
2. **Adapteranschluss:** ARCS liest Telemetrie und führt eine freigegebene
   Bewegung durch den externen Drohnenadapter aus. Ungültige oder nicht
   freigegebene Bewegungen werden abgewiesen. Die beobachtete Wirkung
   wird gespeichert.
3. **Kamera und Wahrnehmung:** Die Drohne liefert Bilder aus ihrer aktuellen
   Sicht. Die Bildauswertung erkennt ein sichtbares Ziel und referenziert
   das verwendete Bild. Ein verdecktes oder außerhalb des Bildes liegendes
   Ziel darf nicht allein durch Simulationswissen gemeldet werden.
4. **Geschlossene Suchschleife:** Die Anwendung nimmt einen Suchauftrag an,
   entscheidet anhand der bisherigen Beobachtungen über weitere Bewegungen
   und sucht bis zum bestätigten Fund oder einer definierten Abbruchbedingung.
5. **Gesamtnachweis:** Wiederholbare Szenarien prüfen mindestens einen
   anfangs unsichtbaren, durch Bewegung auffindbaren Gegenstand und einen
   Raum ohne Zielobjekt. Erfolg und Abbruch sind unterscheidbar; Bilder,
   Entscheidungen, Actions und Wirkungen sind nachvollziehbar gespeichert.

Der erste große Meilenstein ist erreicht, wenn ARCS ein anfangs nicht im
Kamerabild sichtbares Ziel durch eigene Suchbewegungen findet, den Fund
anhand von Kamerabildern bestätigt und kontrolliert stoppt. Bei fehlendem
Ziel endet die Suche innerhalb eines festgelegten Zeit- oder Schrittbudgets
ohne falsche Erfolgsmeldung.

## Nächster Umsetzungsschritt

Die [allgemeinen Adapterfähigkeiten](GENERAL_ADAPTERS.md) und der
[erste Drohnen-Bewegungsnachweis](../../drone-sim/apps/drone-demo/README.md) sind vorhanden.
Die allgemeinen Input- und Reasoning-Anschlüsse sind jetzt auch über stdio
vorhanden. Der neutrale Reasoning-V2-Port unterstützt Solver und Modell-
verfahren. Eine allgemeine Aufgaben-Runtime verarbeitet Inputs und Beobachtungen
nun dauerhaft über konfigurierte Routen mit Wiederaufnahme. Echte KI-Anbieter
bleiben ein weiterer allgemeiner Schritt. Der Stand steht im [Reasoning-Entwurf](REASONING.md).
Der [Kamera-Nachweis](../../drone-sim/adapters/simulated-drone/CAMERA.md) erzeugt inzwischen
perspektivische Bilddaten aus Raum, Pose und Blickrichtung und liefert sie über
den bestehenden Datenreferenzvertrag. Zeit, Zustand, Kamera und Bild gehören
zu einem gespeicherten Snapshot; idealisierte Simulation und unbekannte
Zeitunsicherheit sind sichtbar. Der
[externe Pixel-Reasoner](../../drone-sim/adapters/image-reasoner/README.md)
weist jetzt positive und negative Farbbefunde anhand der übertragenen
Bildbytes nach. Der Suchauftrag geht getrennt an den Reasoner; die Kamera
bleibt zielunabhängig. Allgemeine Binäreingaben für Reasoning V2 liegen in
ARCS, Bildauswertung und Fachverträge ausschließlich extern.

Als Nächstes folgt eine begrenzte Suchschleife in der externen Anwendung:
Nach einem negativen Befund einen Bewegungsvorschlag erzeugen, separat
prüfen und freigeben, ein neues Bild aufnehmen und erneut auswerten.
Schrittbudget und kontrollierter Abbruch gehören zu dieser Etappe.
Die reine Farbsuche ersetzt noch keine semantische Objekterkennung.
