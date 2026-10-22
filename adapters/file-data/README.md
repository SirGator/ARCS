# Allgemeine Dateiquelle

`arcs-file-data` importiert ausdrücklich konfigurierte Dateien als
unveränderliche SQLite-Snapshots. Es veröffentlicht ihre Referenzen über
`data.observe` und liefert deren Bytes über den allgemeinen `read_data`-
Callback. Der Adapter kennt weder Drohnen noch Kameras oder Suchaufträge.
Das Paket hängt vom Protokoll, Adapter-SDK, SQLite und Serde/JSON ab.

## Starten und Daten exportieren

Im Workspace-Verzeichnis:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host -- \
  --config apps/adapter-host/config/data.json
```

Die Ausgabe enthält eine gespeicherte Referenz auf `sample-v1`. Die Datei
liegt zu diesem Zeitpunkt als Snapshot in `var/data-adapter.sqlite`; die
Referenz ist im Core-Store `var/data-host.sqlite` gespeichert.

Das allgemeine Host-Beispiel liest dieselbe persistierte Referenz und
exportiert ihren Inhalt in eine noch nicht vorhandene Zieldatei:

```bash
cargo run --locked -p arcs-host --example export-data -- \
  apps/adapter-host/config/data.json \
  example.files data.observe sample-v1 /tmp/arcs-sample.txt
```

Das Beispiel hat keine Abhängigkeit vom Dateiquellen-Paket. Es startet die
konfigurierten Programme über den allgemeinen Host und verwendet dessen
`copy_data`-API. Das Zielverzeichnis muss existieren. Eine bereits vorhandene
Zieldatei wird nicht überschrieben. Nach einem Neustart kann der Export
unter einem anderen Dateinamen wiederholt werden, ohne neue Observations
oder zusätzliche Core-Artefakte anzulegen.

## Profil und Speicherung

```json
{
  "adapter_id": "example.files",
  "resources": [
    {"id":"sample-v1","media_type":"text/plain","path":"../fixtures/sample.txt"}
  ]
}
```

Ein Profil enthält 1 bis 256 verschiedene Ressourcen. Dateipfade werden
relativ zum Verzeichnis der aufgelösten Profildatei interpretiert.
Der Adapter liest nur diese Betreiberdateien; über das Protokoll sind
ausschließlich opake Ressourcen-IDs abrufbar. Jeder Import ist auf 64 MiB
begrenzt. Das Adapterprogramm erwartet Profil und Datenbank als Argumente:

```text
arcs-file-data <profile.json> <adapter-state.sqlite>
```

Beim Start werden alle Dateien des Profils innerhalb einer SQLite-
Transaktion abgeglichen beziehungsweise importiert. Gleiche ID, gleicher
Medientyp und identische Bytes verwenden den vorhandenen Snapshot weiter.
Andere Bytes oder Metadaten unter einer bestehenden ID führen zu einem
Identitätskonflikt. Neue Inhalte benötigen eine neue ID, auch bei gleicher
Dateigröße. Ein Konflikt rollt den gesamten Import zurück.

Der laufende Adapter liest aus dem Snapshot. Eine spätere Änderung der
Originaldatei verändert deshalb keinen begonnenen Abruf. Beim nächsten
Start wird das Profil erneut eingelesen und mit den Originaldateien
abgeglichen. Die Datenbank ist an die Adapter-ID gebunden. Frühere
Snapshots werden aufbewahrt, auch wenn sie nicht mehr im Profil stehen;
automatische Löschung und ein Gesamtlimit für die Datenbank sind noch nicht
implementiert.

Jeder neue Snapshot erzeugt eine ausstehende Metadaten-Observation.
Unbestätigte Referenzen werden nach Neustarts erneut angeboten; ACKs sind
wiederholbar. Ein ACK beendet das erneute Anbieten und lässt die Bytes
weiterhin abrufbar. Datenbank und Outbox müssen für diese Garantie erhalten
bleiben.

Die Betreiberfreigabe aktiviert `data.observe` und `file.read`. Der Zugriff
auf Inhalte benötigt zusätzlich `data.read` und `data_limits`. Der
mitgelieferte Grant erlaubt 16 MiB pro Objekt und Blöcke von 8192 Bytes.
Die Limits werden vom Host vor dem Datenabruf geprüft; die Adapterprozesse
selbst erhalten durch stdio keine Betriebssystem-Sandbox.

Die Referenz behauptet den Medientyp aus dem Profil. Es findet keine
Dateityperkennung, Bilddecodierung oder Objekterkennung statt. Die gleichen
Verträge können später von einer Kameraquelle implementiert werden.
