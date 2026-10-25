# Aufgaben hauptsächlich als Text eingeben

Die normale Benutzereingabe ist eine Beschreibung des gewünschten Ziels.
Der Input-Adapter transportiert den Originaltext als `arcs.input.v1` mit
`raw_text`. Bedeutung und nötige Schritte bestimmen externe Reasoner. Core,
allgemeiner Host und GUI enthalten keinen fachlichen Textparser. Formulare
und JSON bleiben technische Eingangsmöglichkeiten für Integrationen und Tests.

## Im Kontrollpanel

1. **Aufgaben per Text · Beispiel** auswählen und **ARCS starten**.
2. Unter **Eingabe** die Aufgabe schreiben und **Aufgabe senden** drücken.
3. Im selben Bereich Aufgabenstatus und Zielbewertung verfolgen.

Das Panel öffnet die Eingabeseite. Für neue Installationen steht die
Textkonfiguration zuerst; eine gespeicherte Auswahl bleibt erhalten.
JSON ist als erweiterte Option erreichbar.

Der durchgängige Nachweis versteht beispielsweise:

```text
Schalte den Lüfter ein.
Schalte den Lüfter aus.
Schalte den Lüfter ein und dann wieder aus.
```

`Bitte`, Groß-/Kleinschreibung und einige ausdrücklich implementierte Varianten
sind ebenfalls unterstützt. Es ist ein **begrenzter externer Rules-Reasoner**,
kein allgemeines Sprachmodell. Andere Aufgaben, Negationen und zusätzliche
unbekannte Anweisungen ergeben `no_solution` mit einer Erklärung. „Finde die
rote Kiste mit der Drohne“ wird als nicht unterstützt angehalten und löst
keine Lüfteraktion aus. Die unterstützte Grammatik steht in
[`text_fan.rs`](../../../adapters/example-reasoner/src/text_fan.rs).

Die Interpretation liegt ausschließlich unter `adapters/example-reasoner`,
mit `scenario: "text_fan_goal"` und `method: "rules"`. Andere Reasoner können
denselben Originaltext und ihren ausgewählten Kontext erhalten. Der Harness
schreibt weder eine bestimmte KI-Technik noch einen zentralen Reasoner vor.

## Verarbeitung und Kontrolle

```text
Text → Input-Adapter → gespeicherte Aufgabe → frische Zustandsdaten
     → externer Zielcheck → externer Handlungsvorschlag
     → Verification → Betreiberfreigabe → Act
     → frische Wirkungsdaten → externe Bewertung
     → nächster Schritt oder Abschluss
```

Mit `initial_assessment: true` kann der externe Bewertungs-Reasoner ein bereits
erfülltes Ziel ohne zusätzliche Aktion bestätigen. Vor einer Aktion ist
`effect: "unknown"` zulässig, weil noch keine Wirkung beurteilt wird.
Der Abschluss verweist auf Bewertungskandidat und frische Quellen;
ohne Ausführung entsteht kein Execution-Outcome.

Die Beispielpolicy erlaubt ausdrücklich boolesche Schaltbefehle für `fan-01`
im Simulator. Ein verständlicher Text umgeht keine manuelle Freigabe, wenn
`approval.mode: "manual"` konfiguriert ist. Originaltext, Vorschläge und
Wirkungsdaten bleiben mit ihrer Provenienz gespeichert.

Die Rückmeldung im Textbereich unterscheidet Speicherung im Adapter,
Verarbeitung, Freigabepause, Zielabschluss und angehaltene Aufgaben.
`input_source` im Monitor ordnet eine Zielmeldung über Adapter und
Eingangsreferenz der GUI-Eingabe zu. Mehrere Routen bleiben getrennt.
Die UI erhält dafür keine vollständigen Aufgabenpayloads.

## Konfiguration und CLI

`config/text-tasks/host.json` startet einen leeren Text-Input, einen simulierten
Lüfter sowie getrennte Planner-/Bewertungs-Prozesse. Beim Start gibt es keinen
vorgegebenen Auftrag und keine automatische Schaltaktion. Journale, Core und
Runtime liegen getrennt unter `var/text-tasks/`.

```bash
cargo build --locked --offline --workspace --bins
./target/debug/arcs-host \
  --config apps/adapter-host/config/text-tasks/host.json \
  --runtime apps/adapter-host/config/text-tasks/runtime.json \
  --stop-file /tmp/arcs-text.stop
```

Über `arcs-json-input --enqueue` lautet der Payload beispielsweise
`{"raw_text":"Schalte den Lüfter ein."}`. Ereignis-ID, Subject und Reference
bilden den üblichen äußeren Vertrag. Der Text ist ein Auftrag, keine
Berechtigung und kein Wissen des Observe-Adapters.

Der ältere Eingang `config/text-input/host.json` bleibt zum isolierten
Empfangen und Speichern von Nachrichten verfügbar. Die neue Textaufgaben-
Konfiguration ergänzt die tatsächliche Bearbeitung.
