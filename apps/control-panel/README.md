# ARCS Control

Ein natives GTK-Desktop-Fenster zum Starten und Überwachen des allgemeinen
ARCS-Hosts. Die Oberfläche liegt unter `apps/control-panel`; Core und Adapter
haben keine GUI-Abhängigkeit. Die Kommunikation mit dem eigenen Host-Prozess
läuft lokal über dessen Standardausgabe.

## Start

Im ARCS-Repository:

```bash
./arcs-control
```

Der Starter funktioniert auch aus einem anderen Arbeitsverzeichnis. Fehlen
der Host oder konfigurierte Adapterprogramme, baut das Panel die Rust-Binaries
mit `cargo build --locked --offline --workspace --bins`. Mit **Bauen** kann
der Build ausdrücklich erneut ausgeführt werden.

Benötigt werden Python 3.10 oder neuer, PyGObject und GTK 3 sowie die vorhandene Rust-
Toolchain zum Bauen. Auf dem Entwicklungsrechner sind die Desktop-Bibliotheken
bereits vorhanden. Die Oberfläche benötigt eine erreichbare grafische Sitzung.

Für einen Programmeintrag im persönlichen Anwendungsmenü:

```bash
./arcs-control --install-launcher
```

`--create-launcher` erzeugt stattdessen `ARCS-Control.desktop` im Projektordner.
Die erzeugten Starter enthalten den absoluten Projektpfad. Nach Verschieben
des Checkouts lassen sie sich mit demselben Befehl aktualisieren.

## Bedienung

- **Modelle:** Lokale Modelle und API-Verbindungen direkt einrichten, ohne
  Terminal oder Umgebungsvariablen. Anbieter/Format auswählen, Modellnamen,
  vollständige API-Adresse und gegebenenfalls den maskierten Schlüssel
  eintragen. **Verbindung testen** sendet eine kurze Modellanfrage ohne
  Aufgaben- oder Gerätedaten. **Speichern** legt die Verbindung an. Für
  **Planung** und **Wirkungsprüfung** je ein gespeichertes Modell auswählen
  und **Für Aufgaben übernehmen** drücken. Beide Rollen dürfen dasselbe
  Modell nutzen; mehrere gespeicherte Verbindungen sind möglich. Danach
  unter **Eingabe** ARCS starten und eine Textaufgabe senden. Das vorhandene
  Aufgabenbeispiel steuert den simulierten Lüfter. Neue Fähigkeiten und andere
  Aufgabenabläufe bleiben über Adapter und eigene Konfigurationen anschließbar.
  Während einer laufenden Sitzung sind Änderungen an den Modellen gesperrt.
  Für einen externen Bilddienst gibt es **YOLO · Bildauswertung** und die
  getrennte Zuordnung **Bildauswertung (optional)**. Der Verbindungstest führt
  dort eine echte Bildinferenz aus. Die lokale Umgebung mit Ollama und YOLO
  liegt im Nachbarprojekt [model-services](../../../model-services/README.md);
  Modelle und Gewichte bleiben außerhalb von ARCS. Die Bildfähigkeit ist
  anschließbar, die automatische Kamera- und Drohnenroute ist noch auszubauen.
  Unter **Erweitert** stehen optionale Namen, Schlüsselanforderung,
  Chat-JSON-Modus, Tokenparameter und Anthropic-Version bereit.
- **Startkonfiguration:** Zuerst steht „Aufgaben per Text · Beispiel“ als
  durchgängiger [Textaufgaben-Nachweis](../adapter-host/examples/text-tasks.md).
  Weitere Beispiele sind „Mehrere Adapter“, „Aufgaben & Reasoning“
  sowie „Zielablauf · Lüftertest“ und „Sensor & Lüfter“ und eigene Host-Dateien
  sind auswählbar. Der Zielablauf verwendet eine ausdrückliche Betreiberpolicy
  für das simulierte Gerät; Details stehen im [Nachweis](../adapter-host/examples/goal-runtime.md). Eine Aufgaben-
  Runtime kann separat hinzugefügt oder entfernt werden. Auch Konfigurationen
  externer Projekte sind auswählbar; deren Programme werden in ihrem Projekt gebaut.
- **Aufgaben per Text · LLM-API:** Bleibt als zusätzliche Konfiguration für
  eigene Profile und Umgebungsvariablen verfügbar. Für die normale Einrichtung
  genügt die neue Seite **Modelle**. Die [Anleitung](../adapter-host/examples/llm-tasks.md)
  beschreibt beide Wege und das eigenständige Nachbarprojekt `llm-api`.
- **ARCS starten:** Öffnet den Host und verbindet die konfigurierten Adapter.
  Ohne Aufgaben-Runtime werden konfigurierte Requests und Transforms einmal
  ausgeführt; anschließend werden Inputs und Beobachtungen dauerhaft abgeholt.
  Mit Aufgaben-Runtime gelten deren konfigurierte Routen und Budgets.
- **Übersicht:** Zeigt Sitzungsstatus, Adapterzahl, persistierte Artefakte und
  gegebenenfalls Aufgaben zur Prüfung. Vor dem Start sind Adapter nur
  *konfiguriert*. *Verbunden* bedeutet, dass der Manifest-Handshake gelungen
  ist; es handelt sich nicht um separate laufende Health-Checks.
- **Adapter:** Filter nach Rolle und Name; Details zu Installation, Schemas und
  erlaubten Reasoning-Verfahren. Alle sieben Rollen werden allgemein dargestellt.
- **Eingabe:** Der normale Arbeitsbereich öffnet sich mit der Aufgabenbeschreibung
  als Text. Im Beispiel versteht ein externer Regel-Reasoner etwa „Schalte den
  Lüfter ein“ oder „Schalte den Lüfter ein und dann wieder aus“. Zielmeldungen
  erscheinen im selben Bereich. Verbundenen Eingang auswählen und eine Aufgabe über dessen
  Adapter einreichen. Einfache Pflichtfelder erscheinen als Formular, Texte
  als mehrzeiliges Eingabefeld. Der JSON-Modus erlaubt zusätzliche Felder und
  komplexere Daten. Die Auswahl „Texteingabe“ verbindet einen leeren allgemeinen
  JSON-Input-Adapter für Nachrichten. Ohne passende Runtime-Route werden diese
  gespeichert. „Aufgaben per Text · Beispiel“ verbindet dagegen auch die
  Aufgabensteuerung und externe Beispiel-Reasoner. Weitergehendes
  Sprachverständnis benötigt eine entsprechend leistungsfähige Implementierung.
- **Protokolle:** Prozessmeldungen, Build-Ausgabe und Fehler; als Datei exportierbar.
  Bei Goal-Routen außerdem die tatsächlichen Zielzustände und Schrittzahlen.
- **Stoppen:** Fordert einen geordneten Stopp zwischen den Host-Runden an.
  Auch das Schließen des Fensters wartet zunächst auf diesen Stopp. Nach acht
  Sekunden erscheint bei Bedarf **Sofort beenden**, das ausschließlich die
  eigene Prozessgruppe beendet. Persistierte Journale werden dabei erhalten.

Die zuletzt ausgewählten Pfade liegen unter `var/control-panel/settings.json`.
Die gespeicherte Standardauswahl „Mehrere Adapter“ aus älteren Panel-Versionen
wird einmalig auf „Aufgaben per Text · Beispiel“ umgestellt. Eigene Konfigurationen
und danach ausdrücklich gewählte Beispiele bleiben erhalten.
Eine Sperre verhindert, dass zwei Kontrollpanels dieselbe Core-Datenbank
gleichzeitig starten. Diese Sperre gilt zwischen Kontrollpanels.
Die Oberfläche bietet keine manuellen Approval-Buttons. Freigaben folgen der
Runtime-Konfiguration beziehungsweise dem dokumentierten Betreiberkommando.
Adapter werden weiterhin durch die Host-Konfiguration beim Start angeschlossen.

## Modellverbindungen und Schlüssel

Anbieterbeschreibungen und Verbindungstests übernimmt der externe Adapter
`../llm-api/arcs-llm-api` mit `--describe` und `--probe`. Die GUI enthält keine
Anbieter-API-Aufrufe oder fachliches Reasoning. Ein erfolgreicher Verbindungstest
weist eine kurze vollständige JSON-Modellantwort nach, keine allgemeine
Problemlösungsfähigkeit des Modells. Tests werden nur durch den Testknopf
ausgelöst; beim Öffnen oder Speichern entstehen keine Modellanfragen.

Verbindungsdaten und Zuordnungen liegen ohne Schlüssel in
`var/control-panel/models/connections.json`. Schlüssel bleiben im Speicher
des geöffneten Fensters. Mit optional verfügbarem `libsecret`-Python-Binding
und einem laufenden Secret-Service kann **Schlüssel im Systemschlüsselbund
merken** gewählt werden. Schlüsselbundzugriffe laufen im Hintergrund.
Ohne diese Anbindung zeigt die Oberfläche ausdrücklich die Speicherung nur
für das geöffnete Kontrollpanel an: Nach dessen Neustart ist der Schlüssel
erneut einzugeben. Verbindungen und Zuordnungen bleiben erhalten. Die
Oberfläche schreibt keine Klartext-Schlüsseldatei und übernimmt Keys nicht in
Konfigurations- oder Programmargumente. Der eigene Host bekommt ausschließlich
die für die gewählte Modellkonfiguration benötigten Schlüssel als Umgebung;
angeschlossene Kindprozesse erben diese wie andere Host-Umgebungsvariablen.

**Für Aufgaben übernehmen** erzeugt ein eigenes Konfigurationsverzeichnis
unter `var/control-panel/models/sessions/`. Unterschiedliche Modelle, Anbieter,
Profile und Schemas erhalten getrennte Core-, Runtime- und Adapterjournale.
Schlüsselrotation und Umbenennung behalten die fachlich gleiche Konfiguration.
Die Auswahl erscheint oben als **Meine Modelle · Textaufgaben** und wird wie
andere Startkonfigurationen wiederhergestellt. Generierte Profile und Schemas
sind eingefrorene Kopien; manuell geänderte Dateien werden nicht überschrieben.
Bei einem Anbieter- oder Adresswechsel muss ein Schlüssel ausdrücklich neu
eingegeben werden. Gleichzeitige Änderungen in mehreren Fenstern werden
erkannt; **Neu laden** übernimmt den neueren Stand. Zugeordnete Verbindungen
können erst nach einer anderen gespeicherten Zuordnung entfernt werden.

Die optionalen Schlüsselbundaufrufe folgen den
[libsecret-Python-Beispielen](https://gnome.pages.gitlab.gnome.org/libsecret/libsecret-python-examples.html).

## Eingaben über Adapter

1. Eine Konfiguration mit JSON-Input-Adapter auswählen und ARCS starten.
2. Links **Eingabe** öffnen und den gewünschten Eingang auswählen.
3. Aufgabe als Text schreiben und **Aufgabe senden** drücken. Technische
   Adaptereingänge können weiterhin über Formular oder JSON bedient werden.

Die GUI übernimmt Schema und Größenlimits aus dem tatsächlichen Host-Handshake.
Vor dem Einreihen prüft `arcs-host --validate-input` den Payload mit demselben
Rust-Validator wie der Core. Diese Prüfung startet keine Adapter und öffnet keine
Datenbank. Anschließend verwendet die GUI den bestehenden `arcs-json-input
--enqueue`-Eingang mit dem Profil und Journal der laufenden Installation.
Sie schreibt nicht direkt in den Core-Store. Der normale Input-Poll/ACK-Weg
übernimmt die Aufgabe und erhält Herkunft, Trust, Routing und Freigabegrenzen.

Die Bestätigung „Im Input-Adapter gespeichert“ bedeutet, dass die Adapter-Outbox
die Eingabe dauerhaft angenommen hat. Sie behauptet noch keine Verarbeitung oder
Zielerfüllung. Bei Rückstau oder einem Stopp bleibt die Eingabe im Adapterjournal
für den nächsten Abruf. Ungültige Payloads werden vor der Outbox abgewiesen und
stoppen den laufenden Host nicht. Während einer Übertragung ist der Sendeknopf
gesperrt. Geht eine Bestätigung verloren, verwendet ein unveränderter erneuter
Versuch in derselben GUI-Sitzung dieselbe Ereignis-ID.

Die lokale Einreichungsbrücke unterstützt derzeit `arcs-json-input` mit
`profile` und einem Journalargument in der Host-Konfiguration. Andere externe
Input-Implementierungen besitzen dadurch keine neue Push-Schnittstelle und
werden nicht als beschreibbare GUI-Ziele ausgegeben. Mehrere unterstützte
Input-Adapter sind getrennt auswählbar. Es sind keine zusätzlichen Python-Pakete
für die Schema-Prüfung erforderlich.

## Lokaler Monitor-Vertrag

Das Panel startet den bestehenden Host mit:

```bash
./target/debug/arcs-host --config <host.json> \
  --monitor --stop-file <frischer-stop-pfad> [--runtime <runtime.json>]
```

`ready` enthält den echten Anschlusskatalog und `input_contracts` mit den
aktiven Input-Schemas und Grant-Limits. `snapshot` liefert ungefähr einmal
pro Sekunde Zähler, Runtime-Zustand und begrenzte Zusammenfassungen der letzten
Artefakte. Dabei werden keine vollständigen Artefakt-Payloads übertragen.
Goal-Meldungen enthalten zusätzlich `input_source` (Adapter-ID und Referenz),
damit die Eingabeseite den Fortschritt dem passenden Auftrag zuordnen kann.
`stopped` signalisiert den abgeschlossenen Stopp, `error` einen Fehler.
`--ticks` begrenzt einen Testlauf. Eine vorhandene Stop-Datei wird vor dem
Host-Start abgewiesen. Der Standard-CLI-Lauf ohne `--monitor` bleibt erhalten.

## Prüfen

```bash
cargo test --locked --offline -p arcs-host --test multiple_adapters
python3 -m unittest discover -s apps/control-panel/tests -v
```

Die Desktop-Tests öffnen eigene Testfenster, starten echte Adapter mit isolierten
Daten unter `/tmp` und prüfen Start, Rollenfilter, Details, Aufgabenstatus, Stopp,
Neustart und Fensterschließen. Ohne grafische Sitzung werden nur diese Desktop-
Tests übersprungen. Controller-Tests benötigen keine grafische Sitzung und
prüfen zusätzlich Abbruch eines Builds, Fehler, Datenbanksperre und die Beendigung
eigener Kindprozesse. Eingabetests prüfen echte Outbox/ACK-Übernahme, Text- und
Formulareingaben, mehrere Eingänge, Schemafehler, Neustart und Wiederholung nach
verlorener Empfangsbestätigung. Gerenderte GTK-Ansichten liegen nach dem Desktop-Test unter
`var/control-panel/screenshots`.
