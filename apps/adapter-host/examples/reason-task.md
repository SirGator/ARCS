# Externe Inputs und allgemeines Reasoning

Alle sieben Rollen sind jetzt im stdio-Protokoll und SDK anschließbar.
Dieses Beispiel weist Input und Reason mit drei getrennten Prozessen nach:

```text
json-input → persistierte Aufgabe mit ACK
                   ├→ mathematischer Solver → Lösungskandidat
                   └→ Modell-Testadapter    → anderer Kandidat
```

Die JSON-Eingabe enthält `values: [1, 2, 4]` und `target: 1.8`. Der Solver
enumeriert die Werte und liefert den nächsten Wert **2**. Der ausdrücklich
gekennzeichnete Modell-Testadapter liefert reproduzierbar den ersten Wert
**1**. Er führt keine echte KI-Inferenz aus und beweist keine optimale Lösung.
Beide benutzen denselben Reasoning-V2-Vertrag. Ein echter LLM-/ML-Anbieter
ist noch nicht angebunden.

## Start

Aus dem Cargo-Workspace:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host --example reason-task -- \
  apps/adapter-host/config/roles/host.json example.solver solver
cargo run --locked -p arcs-host --example reason-task -- \
  apps/adapter-host/config/roles/host.json example.model-fixture learned
```

Die Dateien unter `config/roles/` beschreiben Programme, Profile, Grants und
Fachschemas. Getrennte Core-/Adapterdatenbanken liegen ignoriert unter
`var/roles-*.sqlite`. Relative Dateipfade beziehen sich auf die Hostdatei;
absolute Konfigurationspfade erlauben den Aufruf aus anderen Verzeichnissen.

Das Beispiel wählt den ersten gespeicherten Task von `example.input` und
übergibt nur die Felder `values` und `target`. `private_note` verlässt den
Core nicht. Die Request-ID bindet diesen Auftrag und wird pro Capability
dedupliziert. Wiederholungen verwenden dieselben Audit- und Kandidatenversionen.
Neue Aufgaben benötigen neue Ereignis-/Auftragsidentitäten.

## Input

`PollInput`, `Input`, `AckInput` und `InputAcknowledged` sind getrennt von
Observe und dessen ACK. Der Core prüft Role, Grant, Schema und Größenlimits,
speichert das Ereignis samt Empfangsbeleg atomar und sendet erst dann ACK.
Eine verlorene Bestätigung führt bei Wiederholung zum selben Artefakt.
Geänderte Daten unter derselben Message-ID werden nicht bestätigt.

Input erzeugt historische Ereignisse, keinen World-State und keinen
Current-State-Slot. Ein Auftrag autorisiert weder Reasoning-Kosten noch
eine Aktion von selbst. `Host::poll_inputs` gibt gespeicherte Ereignisse
an die Anwendung zurück; `Host::run` pollt sie zusätzlich in jeder Runde.
`polling.max_inputs_per_adapter` begrenzt die Anzahl pro Adapter/Runde
(Standard 1000). Ein erreichtes Limit steht unter `input_limit_hits`.
Die Konfiguration muss zum angebotenen Input-Profil passen; nicht
freigegebene Capabilities werden sichtbar abgewiesen.

Die mitgelieferte JSON-Quelle startet mit einer festen Ereignisliste und SQLite-Outbox.
Mit `--enqueue` können neue Ereignisse während des Betriebs hinzugefügt werden.
Sie ist kein HTTP- oder Chatserver. Änderungen am Profil brauchen ein neues
Journal beziehungsweise eine neue Installation mit passenden stabilen IDs.

## Reasoning V2

`Host::reason(ReasoningRequestV2)` adressiert eine konkrete Capability und
Methode: `solver`, `rules`, `llm`, `learned` oder `hybrid`. Der neue Dienst
ist `GeneralReasoningService`; die alten Modell-DTOs und V1-Dienste bleiben
erhalten. Auf stdio heißen die Nachrichten `ReasonV2` und
`ReasoningResultV2`, der SDK-Callback `reason_v2`.

Ein Betreiber-Grant enthält `reasoning_limits_v2` mit zugelassenen Methoden
und Ressourcenlimits. Modellbasierte Methoden erfordern `producer_class:
model`; ein Name wie „Solver“ ersetzt diese Zuweisung nicht. Solver und
Regeln benötigen ein Iterationsbudget, LLMs ein Tokenbudget. Kontext-,
Ergebnis-, Kandidaten- und Laufzeitlimits gelten allgemein. Die Hostfrist
ist das Minimum aus Runtime-Budget und dem konfigurierten stdio-Timeout.
Ein hängender Prozess wird beendet; es gibt keinen automatischen Fallback.
Backendseitige Token-/Iterationsgrenzen bleiben Treiberverantwortung; der
Core prüft außerdem die berichteten Verbrauchszähler.

Der Core validiert die gesamte Antwort vor dem Speichern. Er akzeptiert nur
das angeforderte Candidate-Schema, ausgewählte Evidenzreferenzen und
ausdrücklich erlaubte Fähigkeiten. Ein Candidate enthält keine Approval-
oder Ausführungsbefugnis. Auch ein mathematisch erzeugter Kandidat braucht
bei späteren Wirkungen weiterhin Verification und Approval.

Modell- und modellabgeleitete Inhalte bleiben auf Modellherkunft und
niedrigem Trust. Der Solver erhöht den Trust seiner Eingabe nicht. Aktuelle
Grants, Ressourcen und Trust-Grenzen werden auch vor Replay geprüft.
Die Herkunftsspur liegt ohne erfundene Prompts oder Temperaturen im
gespeicherten V2-Ergebnis; sie ist kein unabhängiger Korrektheitsbeweis.

Audit-Auftrag und Antwort verwenden neue Schema-IDs
`arcs.reasoning_request.v2` und `arcs.reasoning_result.v2`; V1-Verträge
werden nicht umgedeutet. Antworten und Kandidaten werden dauerhaft
dedupliziert. Ein gespeichertes Ergebnis kann nach einem Abbruch die noch
fehlenden Kandidaten idempotent nachtragen. Im Status `no_solution` oder
`budget_exhausted` entstehen keine Kandidaten. Das sind Aussagen des
jeweiligen Backends, keine unabhängig bewiesenen Eigenschaften des Problems.

## Rollen und Runtime

Ein Adapter kann weiterhin mehrere Rollen und Fähigkeiten in einem Prozess
anbieten. Der SDK-Test `tests/roles.rs` nutzt Input und Reason gemeinsam;
die Sensor-/Lüfter-/Drohnenbeispiele bleiben unverändert anschließbar.

Dieses Beispiel führt begrenzte synchrone Dialoge aus und wählt Reasoner
explizit. Die [Aufgaben-Runtime](task-runtime.md) ergänzt einen Dauerlauf
mit konfigurierten Input-/Observe-Routen, persistenter Queue und Review-Übergabe.
Asynchrone Planung, parallele Reasoner-Auswahl und automatische Kombination
mehrerer Ergebnisse sind noch nicht implementiert.

```bash
cargo test --locked -p arcs-host --test input_reasoning
cargo test --locked -p arcs-adapter-sdk --test roles
```
