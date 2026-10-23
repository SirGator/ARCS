# Erstes großes Anwendungsziel: Drohnensuche im simulierten Raum

## Festgelegtes Ziel

ARCS soll eine simulierte Drohne durch einen simulierten Raum steuern und
dabei mit der Drohnenkamera ein vorgegebenes Zielobjekt finden.

Die bestehende Trennung gilt weiter: Der Core enthält allgemeine
Artefakt-, World-, Reasoning-, Verification- und Action-Logik. Konkrete
Geräteadapter und Simulationscode liegen außerhalb des Core.

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

Noch nicht vorhanden sind eine Raum- und Drohnensimulation, eine Kamera,
Bildwahrnehmung, ein Suchauftrag und eine fortlaufende Suchsteuerung.
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

Diese Vereinfachungen sind ein Umsetzungsvorschlag. Simulationsengine,
Bildformat, Erkennungsverfahren und konkretes Zielobjekt sind noch offen.

## Geplante Zuständigkeiten

Die folgenden Komponenten sind geplant, noch nicht implementiert:

| Ort | Verantwortung |
| --- | --- |
| `simulation/` | Raum, Drohnenzustand, Bewegungen, Kollisionen und Kamerabilder |
| `adapters/simulated-drone/` | Protokollzugang zur Simulation, Telemetrie, Kameraabfragen und Bewegungsbefehle |
| `adapters/camera-perception/` | Bildanalyse mit referenziertem Eingangsbild, Fundkandidaten und beobachteten Bildpositionen |
| `apps/drone-search/` | Suchauftrag, Suchstrategie, Verifikation, Freigabepolicy, Ablauf und Fundbericht |
| `crates/arcs-core/` | Allgemeine Verträge und Kontrollmechanismen, soweit für den Durchstich erforderlich |

Simulation und Drohnenadapter können anfangs in einem Prozess laufen.
Ihre Verantwortung bleibt getrennt. Auch die Kamerawahrnehmung kann zunächst
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

Zuerst werden die [allgemeinen Adapterfähigkeiten](GENERAL_ADAPTERS.md)
ausgebaut und an bestehenden Beispielen nachgewiesen. Die gemeinsame
Adapter-Session liegt jetzt im geräteunabhängigen `arcs-adapter-sdk`;
Sensor und Lüfter verwenden sie bereits. Der Drohnensimulator wird erst
anschließend an diese Grundlage angebunden.

Bei der späteren Erprobung liefert der Drohnenanschluss Telemetrie und
Bewegungsfähigkeiten über diese allgemeinen Mechanismen. Der erste
drohnenspezifische Nachweis lautet dann: ARCS erhält eine Observation,
führt eine freigegebene Bewegung aus und beobachtet die veränderte Pose.
Darauf folgen Kamerabild, Bildauswertung und die vollständige Suche.
