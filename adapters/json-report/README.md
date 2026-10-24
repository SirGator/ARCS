# Allgemeiner JSON-Berichtsadapter

`arcs-json-report` stellt deklarierte JSON-Payloads als Bericht dar. Er nutzt
den allgemeinen Transform-Port und hängt nur von Protokoll, SDK und Serde/JSON
ab. Es gibt keine Core-, Sensor- oder Drohnenabhängigkeit.

```bash
cargo build --locked -p arcs-json-report
./target/debug/arcs-json-report adapters/json-report/config/profile.json
```

Das Programm spricht JSON-Lines über stdin/stdout; Logs gehen nach stderr.
Im normalen Einsatz startet der Host diesen Prozess.

## Vertrag

Ein Profil enthält eine Adapter-ID und die ausdrücklich akzeptierten
Schema-IDs:

```json
{
  "adapter_id":"example.report",
  "accepts":["arcs.measurement.temperature.v1","arcs.number.normalized.v1"]
}
```

Die Transform-Capability `json.report` erfordert `data.transform` und liefert
`arcs.report.v1`, den bestehenden Vertrag der Dateiausgabe. Alle im Profil
genannten Schemas müssen beim Host geladen sein. Die fachliche
Schema-Validierung und die Zuweisung von Herkunft und Trust erfolgen im Core.
Andere Fachverträge lassen sich durch eine andere `accepts`-Liste anschließen.

Seit Adapterversion `0.2.0` gibt es zusätzlich `report.compact`: Dieser
Transform akzeptiert `arcs.report.v1`, liest dessen Text als JSON und entfernt
unnötige Formatierungsleerzeichen. Titel und JSON-Inhalt bleiben erhalten;
ein Bericht mit gewöhnlichem Fließtext wird abgewiesen. Er benötigt ebenfalls
`data.transform` und ein eigenes aktiviertes Capability-Recht im Grant.
Beide Fähigkeiten können im selben Prozess oder über separate Installationen
genutzt werden. Das Pipeline-Beispiel verkettet Darstellung und Komprimierung.

Der Bericht hat den festen Titel `JSON artifact`. Sein Text ist formatiertes
JSON mit `schema_id`, `version_id` und dem unveränderten `payload` der Eingabe.
Der Adapter interpretiert keine Messwerte und holt keine referenzierten
Binärdaten nach. Das ist eine Darstellung mit Evidenzreferenz, keine
inhaltliche Zusammenfassung, Bildanalyse oder Bestätigung der Daten.

Der Berichtstext ist auf 60 KiB begrenzt; der komplette JSON-Payload muss
zusätzlich in `max_output_bytes` des Auftrags passen. Die Darstellung kann
größer als der Eingang sein. Ein überschrittenes Limit wird abgewiesen.
Für dieselbe Eingabe bleibt das Ergebnis deterministisch. Der Core übernimmt
persistentes Replay, Provenienz und die Trust-Grenze. Der Adapter erzeugt
keine Verification, Freigabe oder externe Ausgabe.

Den vollständigen Prozessweg zeigt das
[Pipeline-Beispiel](../../apps/adapter-host/examples/observe-report/README.md).
