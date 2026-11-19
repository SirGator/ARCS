# Echte Inferenz, Erinnerungskontext und persistente Goal-Wiederaufnahme

Der ausführbare Nachweis verbindet den vorhandenen externen LLM-Adapter mit
dem normalen ARCS-Host. Die Aufgabe lautet: „Schalte den Lüfter ein und dann
wieder aus.“ Der Core enthält weiterhin keine Sprachinterpretation oder
Modellinferenz. Planung und Bewertung kommen aus zwei externen Reason-Prozessen;
der Aktor ist der vorhandene Lüftersimulator.

Zwischen den zwei Schritten simuliert die Runtime **90 Tage**. Der Nachweis
startet den Host mehrfach neu und prüft gespeicherte Aufgaben, frische Quellen,
neue Freigaben und unveränderte abgeschlossene Wirkungen. Das ist kein Nachweis
von 90 Tagen tatsächlicher Betriebszeit.

## Vorbereitung

Die Nachbarprojekte `../llm-api` und `../model-services` müssen vorhanden sein.
Der LLM-Adapter benötigt die Referenzpolicy aus lokalem Commit `a2475cf`.
Da das Nachbarrepository keinen Remote besitzt, ist die vollständige Änderung
auch als [anwendbarer Patch](patches/llm-api-reference-policy.patch) enthalten.
Für einen unveränderten Adapterstand `3c7eca3` lässt sie sich im ARCS-Verzeichnis
zunächst prüfen und anschließend anwenden:

```bash
git -C ../llm-api apply --check "$PWD/apps/adapter-host/examples/patches/llm-api-reference-policy.patch"
git -C ../llm-api apply "$PWD/apps/adapter-host/examples/patches/llm-api-reference-policy.patch"
```

Bei bereits vorhandenem Commit diesen Schritt auslassen.
Die Modelllaufzeit und Gewichte bleiben im externen Modellprojekt. Ein lokaler
Ollama-Dienst mit dem gewählten Modell muss erreichbar sein. Mit der vorhandenen
Modellumgebung sind beispielsweise diese Schritte möglich:

```bash
cd ../model-services
./model-services start
./model-services pull --model qwen2.5:14b
cd ../ARCS
cargo build --workspace --bins --examples --locked --offline
mkdir -p var/live-llm
```

`model-services start` startet seine konfigurierten Modellprozesse; ein bereits
anderweitig betriebener Ollama-Dienst kann ebenfalls verwendet werden. Für
Installation und Betrieb gilt die [externe Modellanleitung](../../../../model-services/README.md).
Modell-Downloads und Inferenz erfolgen ausdrücklich außerhalb des Core.

## Ausführen

```bash
ARCS_LLM_MODEL=qwen2.5:14b \
ARCS_LLM_ENDPOINT=http://127.0.0.1:11434/api/chat \
./target/debug/examples/persistent-goal --approve-simulated-actions \
  apps/adapter-host/config/live-llm/host.json \
  apps/adapter-host/config/live-llm/runtime.json \
  > var/live-llm/qualification-report.json
```

Provider, Modellprofile, Rollen und Journale sind getrennt konfiguriert.
Die neue Konfiguration verwendet ausschließlich Ollama und reicht den beiden
Modellprozessen nur Modellname und Endpunkt als zusätzliche Umgebungsvariablen
weiter. API-Schlüssel werden dafür nicht benötigt oder übernommen.
Ein Modell-/Profilwechsel erfordert neue Journale.

Das Flag `--approve-simulated-actions` erlaubt ausschließlich die Testfreigaben
des Beispielrunners. Dieser prüft vor dem Hoststart, dass die Action-Installation
auf das bekannte `arcs-simulated-fan`-Programm desselben Cargo-Profils zeigt.
Ein anderes Aktorprogramm wird abgewiesen. Die Runtime verlangt weiterhin die
einzelne Review-Entscheidung für jede Candidate-Version. Der Runner prüft
zusätzlich den erwarteten booleschen Schritt und das beobachtete Zielgerät,
bevor er die Testentscheidung setzt. Es findet keine menschliche Einzelprüfung
statt; die Freigaben sind ausdrücklich Teil des isolierten Simulatornachweises.
Der normale Host erhält dadurch keine neue automatische Freigabepolicy.

Die Beispieljournale liegen unter `var/live-llm/14b-bound/`. Der Runner
verlangt einen frischen Aufgabenbestand. Ein weiterer Aufruf auf einem bereits
benutzten Bestand erhält diesen und meldet einen Fehler. Für unabhängige
Versuche eigene Host-/Runtime-Konfigurationen mit getrennten Datenpfaden
verwenden. Die Wiederaufnahmeprüfung ist Bestandteil des ersten Durchlaufs.

Die Profile aktivieren `reference_policy: "context"`. Der externe Adapter
bindet die gelieferten Kontextversionen als Transportprovenienz, statt das
Modell lange Versions-IDs abschreiben zu lassen. Diese Liste belegt die
übergebenen Eingaben; sie behauptet nicht, dass das Modell jede Quelle
inhaltlich berücksichtigt oder bestätigt hat.

Für den Assessor bindet `context_reference_fields` zusätzlich ausschließlich
`evidence_version_ids` aus den ausdrücklich konfigurierten
`/constraints/goal_runtime/before`- und `after`-Verweisen. Alle IDs müssen
bereits als Artefakte im Modellkontext vorliegen. Das Feld wird aus dem
modellseitigen Antwortschema entfernt und anschließend vom Adapter ergänzt.
Die Ziel-Schemata und der Core-Vertrag bleiben unverändert. Das Modell liefert
weiterhin sämtliche fachlichen Aussagen, insbesondere `effect`, `goal_status`
und die vorgeschlagenen Aktionen. Der Adapter schreibt keine Urteile um.
Trace und journalgebundene Profilidentität dokumentieren diese Policy.

Ohne explizite Policy bleibt der bisherige strikte Referenzvertrag erhalten.
Die ebenfalls verfügbare Policy `deduplicate` entfernt lediglich doppelte
Nennungen bekannter IDs; unbekannte IDs werden weiterhin zurückgewiesen.

## Geprüfter Ablauf

| Checkpoint | Prüfaussage |
| --- | --- |
| `first_review` | Initialer Zielcheck und Planung liefern einen prüfbaren Vorschlag; noch keine Wirkung |
| `first_review_reopened` | Neustart erhält dieselbe Candidate- und Verification-Version; keine neue Inferenz |
| `waiting` | Erste freigegebene Aktion ist frisch als eingeschaltet mit genau einem Zustandswechsel beobachtet |
| `waiting_reopened_before_due` | Neustart und Warte-Ticks vor dem gespeicherten Termin lösen keine neue Inferenz aus |
| `second_review` | Nach dem simulierten Termin folgen frische Quellen, ein neuer Vorschlag und eine eigene Freigabe |
| `completed` | Frische Beobachtung zeigt ausgeschaltet und genau zwei Zustandswechsel; Core-Outcome meldet Erfolg |
| `completed_reopened` | Ein weiterer Neustart erhält dieselben Ausführungs-/Outcome-Versionen und Versuchsbelege |

Die verspätete Freigabe der ersten Candidate-Version für die zweite Runde
wird ausdrücklich getestet und zurückgewiesen. Während der geprüften
Wartepunkte sind die benannten Rechenressourcen frei. Die exklusive
Kontrollsitzung verwendet den bestehenden Core-Reservierungsdienst.

Für einen erfolgreichen Durchlauf werden sechs gültige Reasoning-Versuche
erwartet: je Runde initialer Check, Planung und Wirkungsbewertung. Jeder
gespeicherte Modellauftrag muss die ursprüngliche Aufgabenversion und das
freigegebene `body` der Betreiber-Notiz enthalten. Das Feld `private_note`
gehört nicht zum Kontext. Der Nachweis behauptet keinen Qualitätsgewinn durch
diese Hinweisnotiz und keine automatische Erfahrungskonsolidierung.

Das Ergebnis ist JSON mit `passed`, einem eventuellen `error`, dem Core-
Datenbankpfad, simulierter Zeit, tatsächlicher Laufzeit, Checkpoints und den
Versionen der Versuchsbelege. `adapter_reported_models` kennzeichnet die vom
Adapter berichteten Implementierungs-/Modellnamen. Diese Angaben sind keine
kryptographische Modellattestation oder kalibrierte Qualitätsbewertung.
Fehlgeschlagene Läufe liefern `passed: false` und Exitcode 1. Ihre Journale
bleiben für die Ursachenprüfung erhalten; Antworten werden nicht passend gemacht.

## Grenzen und Prüfbudgets

Die Operator-Konfiguration begrenzt jeden Modellaufruf auf fünf Minuten und
2.048 Ausgabetokens. Das Host-Protokoll hat einen gesonderten Timeout von
305 Sekunden; jede Nachweisphase ist auf 750 Sekunden tatsächliche Zeit
begrenzt. Zwei Reason-Prozesse teilen einen benannten Rechenplatz. Quellenabfragen
verwenden zusätzlich das getrennte [Request-Workerbudget](parallel-requests.md). Die
Gesamtaufgabe hat eine maximale simulierte Lebensdauer von 120 Tagen.

`tick_at` verändert die Scheduler-Zeit. Modell-Timeouts und tatsächliche
Laufzeitmessung bleiben an die reale Zeit gebunden. Die Core-Clock für
Artefaktzeitstempel wird dadurch nicht verstellt. Der Modelldienst kann
Gewichte weiterhin im Speicher halten; freie Host-Rechenplätze sind keine
Aussage über freigegebenen RAM.

Der Fall deckt einen bekannten Simulator und eine kurze geordnete Aufgabe ab.
Unbekannte Aufgaben ohne konfigurierte Routen, allgemeine Neuplanung,
Monatsbetrieb unter realer Zeit, verteiltes Controller-Fencing und autonome
Drohnenmissionen sind andere Abnahmen.

Die normale Regression führt denselben Runner mit deterministischen externen
Rules-Prozessen aus. Sie prüft Wiederaufnahme, Evidenz, Ablehnungen und die
Begrenzung der Testfreigaben. Diese Tests werden nicht als echte Modellinferenz
ausgegeben:

```bash
cargo test -p arcs-host --test persistent_goal_qualification --locked --offline
```
