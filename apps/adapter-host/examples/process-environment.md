# Prozessumgebung und Schlüssel

Diese Umsetzung folgt [ARCHITECTURE.md, §10.1 SEC-05](../../../docs/ARCHITECTURE.md):
Modellschlüssel werden nur an ausdrücklich konfigurierte Adapter weitergegeben.
Die allgemeine Host-Schicht kennt dabei keine Anbieter oder Gerätemodelle.

## Konfiguration

`arcs-host` entfernt beim Start jedes Adapters zunächst die geerbte Umgebung
und übernimmt die vorhandenen Variablen aus
[`environment-base.json`](../config/environment-base.json). Diese gemeinsame
Basisliste enthält Suchpfade, Benutzer-/Home-Angaben, Locale, Zeitzone und
temporäre Verzeichnisse. Sie enthält keine Modellschlüssel, Proxy- oder
Loader-Konfiguration.

Zusätzliche Variablen werden je Adapter nach Namen freigegeben:

```json
{
  "program": "../../../target/debug/arcs-external-reasoner",
  "args": [],
  "grant_file": "grants/planner.json",
  "inherit_env": ["PLANNER_API_KEY", "PLANNER_MODEL"]
}
```

Die Werte stammen aus der Umgebung des gestarteten Hosts. Sie gehören nicht
in die Konfiguration, Argumente oder das Artefaktgedächtnis. Ein anderer
Adapter erhält diese Variablen nur mit einer eigenen Freigabe.

`inherit_env` ist standardmäßig leer. Erlaubt sind höchstens 64 eindeutige
Namen mit jeweils 1 bis 128 ASCII-Zeichen nach
`[A-Za-z_][A-Za-z0-9_]*`. Wildcards sind nicht erlaubt. Nicht vorhandene
Variablen bleiben auch im Kindprozess abwesend. Ungültige Listen werden vor
dem Start irgendeines Adapters zurückgewiesen.

Adapter, die zusätzliche Proxy-, Zertifikats-, Bibliotheks- oder
Python-Variablen benötigen, müssen deren Namen ebenfalls ausdrücklich
aufführen. Bestehende Konfigurationen, die von pauschaler Vererbung abhingen,
benötigen diese Ergänzung.

## Modellkonfiguration und Kontrollpanel

Das manuelle Beispiel `config/llm-tasks/host.json` gibt den beiden
Modelladaptern die unterstützten Provider- und Modellvariablen frei. Diese
Liste kann auf den tatsächlich verwendeten Provider eingeschränkt werden.
Input- und Lüfteradapter erhalten keine zusätzlichen Variablen.

Die im Kontrollpanel erzeugten Konfigurationen sind enger: Jeder
Modelladapter erhält nur die Schlüsselvariable seines zugeordneten Modells.
Der Host selbst erhält die ausdrücklich deklarierten Umgebungsvariablen und
die ausgewählten Schlüssel. Ein Verbindungstest erhält nur seinen temporär
ausgewählten Schlüssel; die Adapterbeschreibung benötigt keinen Schlüssel.
Lokale Eingabevalidierung und das Einreihen von Eingaben erhalten nur die
Basisumgebung. Der Cargo-Build verwendet weiterhin die Toolchain-Umgebung.

Ältere generierte Cloud-Konfigurationen ohne Prozessfreigaben müssen unter
**Modelle → Für Aufgaben übernehmen** erneut erzeugt werden. Sie werden
nicht stillschweigend verändert: Alte Dateien und Journale bleiben bestehen.
Die neue Konfiguration hat einen eigenen Stand; spätere Schlüsselrotation
behält innerhalb dieses Standes die Konfigurationsidentität.

## Grenze und Nachweis

Die Begrenzung verhindert die unbeabsichtigte Weitergabe von Umgebungswerten.
Sie ist eine Teilimplementierung von SEC-05. Sie ersetzt keine
Betriebssystem-Sandbox, Dateizugriffskontrolle oder Prozessisolation gemäß
SEC-06. Direkte Nutzer des Core-`StdioAdapter` außerhalb dieses Hosts bestimmen
ihre Prozessumgebung selbst.

```bash
cargo test --locked -p arcs-host --test process_environment
python3 -m unittest discover -s apps/control-panel/tests -p test_process_environment.py -v
python3 -m unittest discover -s apps/control-panel/tests -p test_models.py -v
python3 -m unittest discover -s apps/control-panel/tests -p test_controller.py -v
```

Echte Kindprozesse prüfen mit künstlichen Schlüsseln die Trennung zwischen
Adaptern, fehlende Variablen, Neustart und die Ablehnung ungültiger Freigaben.
Die Testberichte enthalten boolesche Ergebnisse statt Schlüsselwerten.
Die LLM-Integrationstests prüfen zusätzlich die unterstützten Providerformate
mit lokalen HTTP-Testservern und künstlichen Schlüsseln.
