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

- **Startkonfiguration:** Die Beispiele „Mehrere Adapter“, „Aufgaben & Reasoning“
  und „Sensor & Lüfter“ sowie eigene Host-Dateien sind auswählbar. Eine Aufgaben-
  Runtime kann separat hinzugefügt oder entfernt werden. Auch Konfigurationen
  externer Projekte sind auswählbar; deren Programme werden in ihrem Projekt gebaut.
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
- **Protokolle:** Prozessmeldungen, Build-Ausgabe und Fehler; als Datei exportierbar.
- **Stoppen:** Fordert einen geordneten Stopp zwischen den Host-Runden an.
  Auch das Schließen des Fensters wartet zunächst auf diesen Stopp. Nach acht
  Sekunden erscheint bei Bedarf **Sofort beenden**, das ausschließlich die
  eigene Prozessgruppe beendet. Persistierte Journale werden dabei erhalten.

Die zuletzt ausgewählten Pfade liegen unter `var/control-panel/settings.json`.
Eine Sperre verhindert, dass zwei Kontrollpanels dieselbe Core-Datenbank
gleichzeitig starten. Diese Sperre gilt zwischen Kontrollpanels.
Die Oberfläche erteilt keine Verification-, Approval- oder Aktionsfreigaben.
Adapter werden weiterhin durch die Host-Konfiguration beim Start angeschlossen.

## Lokaler Monitor-Vertrag

Das Panel startet den bestehenden Host mit:

```bash
./target/debug/arcs-host --config <host.json> \
  --monitor --stop-file <frischer-stop-pfad> [--runtime <runtime.json>]
```

`ready` enthält den echten Anschlusskatalog. `snapshot` liefert ungefähr einmal
pro Sekunde Zähler, Runtime-Zustand und begrenzte Zusammenfassungen der letzten
Artefakte. Dabei werden keine vollständigen Artefakt-Payloads übertragen.
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
eigener Kindprozesse. Gerenderte GTK-Ansichten liegen nach dem Desktop-Test unter
`var/control-panel/screenshots`.
