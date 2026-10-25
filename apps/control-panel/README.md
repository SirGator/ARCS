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

- **Startkonfiguration:** Zuerst steht „Aufgaben per Text · Beispiel“ als
  durchgängiger [Textaufgaben-Nachweis](../adapter-host/examples/text-tasks.md).
  Weitere Beispiele sind „Mehrere Adapter“, „Aufgaben & Reasoning“
  sowie „Zielablauf · Lüftertest“ und „Sensor & Lüfter“ und eigene Host-Dateien
  sind auswählbar. Der Zielablauf verwendet eine ausdrückliche Betreiberpolicy
  für das simulierte Gerät; Details stehen im [Nachweis](../adapter-host/examples/goal-runtime.md). Eine Aufgaben-
  Runtime kann separat hinzugefügt oder entfernt werden. Auch Konfigurationen
  externer Projekte sind auswählbar; deren Programme werden in ihrem Projekt gebaut.
- **Aufgaben per Text · LLM-API:** Verbindet externe API-Reasoner aus dem
  Nachbarprojekt `llm-api`. Vor dem Panel-Start Modell, Anbieter und gegebenenfalls
  API-Zugang im startenden Terminal konfigurieren. Die [Anleitung](../adapter-host/examples/llm-tasks.md)
  beschreibt den Start; bestehende Fenster erben neue Umgebungsvariablen nicht.
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
