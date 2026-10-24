# ARCS Adapter-Protokoll: stdio v1

Die gemeinsame Implementierung liegt im eigenständigen Paket
[`crates/arcs-adapter-protocol`](crates/arcs-adapter-protocol). Sie enthält
Nachrichten, IDs, Manifestprüfung und Framing und hat keine Core-Abhängigkeit.
Das [`arcs-adapter-sdk`](crates/arcs-adapter-sdk/README.md) ergänzt für
Rust-Adapter eine gemeinsame Session mit typisierten Input-, Observe-, Request-,
Transform-, Reason-, Output- und Act-Callbacks. Sensor und Lüfter verwenden diese Implementierung. Das
Protokoll bleibt auch ohne das SDK und in anderen Sprachen implementierbar.
Die konkreten Adapter liegen unter `adapters/`, ihre Beispiel-Hosts unter
`apps/demo-host`. Grants, Registry und die Dienste für kontrollierte
Ausführung liegen in `crates/arcs-core`.

Der allgemeine Host unter `apps/adapter-host` lädt ausführbare Programme,
Profile, Schemas, Grants und Abfragen aus JSON. Seine
[Konfiguration und Startbefehle](apps/adapter-host/README.md) funktionieren
mit beiden Simulatoren ohne gerätespezifischen Host-Code. Die folgenden
Demo-Hosts bleiben Beispiele für die gezielte fachliche Orchestrierung.

Dieser erste Transport verbindet einen vom Betreiber gestarteten Adapterprozess
mit den vorhandenen Core-Diensten. Implementiert sind Registrierung,
bestätigte Inputs und Observations, Requests, Transforms, allgemeines Reasoning V2,
Outputs, Actions und der begrenzte Abruf referenzierter Daten über die Rust-Dienste.
Die bisherigen internen Input- und Reasoning-V1-Schnittstellen bleiben erhalten.

## Prozessgrenze und Berechtigungen

Der Host startet eine explizit konfigurierte ausführbare Datei mit getrennten
Argumenten. Die Identität der lokalen Verbindung ergibt sich aus diesem
Prozessstart und dem vom Betreiber bereitgestellten `AdapterGrant`. Das vom
Child gelieferte Manifest muss zu diesem Grant passen. Ein Manifest erteilt
keine Rechte. Schemas lädt und registriert der Host selbst.

Ein Reconnect an dieselbe Registry ist mit identischem Manifest und Grant
möglich. Änderungen benötigen eine explizite neue Installation; sie werden
nicht beim Wiederverbinden übernommen.

stdin und stdout transportieren ausschließlich Protokollnachrichten. Logs gehen
nach stderr. stdio stellt keine Betriebssystem-Sandbox bereit. Der erste Slice
enthält keine Netzwerk-Authentifizierung oder Verwaltung von Prozessgruppen.
Das direkte Child wird bei Shutdown, Fehlern und beim Freigeben des Handles
beendet und eingesammelt.

## Framing und Envelope

Jede Nachricht ist ein UTF-8-JSON-Objekt auf genau einer Zeile, abgeschlossen
mit LF. Ein Frame darf einschließlich LF höchstens **65.536 Bytes** groß sein.
Unbekannte Felder, ungültiges JSON, unvollständige Frames und nicht unterstützte
Versionen werden abgewiesen. Zusätzlich gelten die kleineren Payload- und
Referenzlimits des Grants.

```json
{"protocol_version":1,"request_id":"stdio-1","message":{"type":"hello"}}
```

Die Antwort wiederholt `request_id` exakt und verwendet `protocol_version: 1`.
Es gibt nur einen offenen Request je Verbindung. Der Host vergibt fortlaufende
Request-IDs; diese gelten nur für die jeweilige Verbindung. Die Deadline gilt
für Schreiben und Lesen eines Dialogschritts; die Demo verwendet fünf Sekunden.
Ein Timeout beendet die Verbindung. Es gibt keine versteckten automatischen
Retries. Der Host entscheidet über Reconnect und erneute Verarbeitung.

## Nachrichtenfolge

| Host → Adapter | Adapter → Host | Bedeutung |
| --- | --- | --- |
| `hello` | `manifest` mit `manifest` | Fähigkeiten anbieten und Core-seitig prüfen |
| `poll_input` | `input` mit `message_id` und `input` | Nächste Aufgabe oder nächstes Ereignis anbieten |
| `poll_input` | `idle` | Zurzeit kein Input |
| `ack_input` mit `message_id` | `input_acknowledged` mit derselben `message_id` | Dauerhaften Input-Commit bestätigen |
| `reason_v2` mit `invocation` | `reasoning_result_v2` mit `response` | Begrenzten allgemeinen Reasoning-Auftrag beantworten |
| `poll` | `observation` mit `message_id` und `observation` | Nächste Messung anbieten |
| `poll` | `idle` | Zurzeit keine Messung; Verbindung bleibt offen |
| `ack` mit `message_id` | `acknowledged` mit derselben `message_id` | Erfolgreichen Store-Commit bestätigen |
| `request` mit `invocation` | `request_result` mit `response` | Gezielte Datenanforderung über `RequestService` |
| `transform` mit `invocation` | `transform_result` mit `response` | Deterministische Ableitung über `TransformService` |
| `output` mit `invocation` | `output_result` mit `response` | Geprüften und freigegebenen Ausgabeauftrag liefern |
| `execute` mit `invocation` | `execution_result` mit `response` | Freigegebenen Action-Auftrag über `ExecutionService` ausführen |
| `read_data` mit `request` | `data_chunk` mit `chunk` | Referenzierte Daten unter den Limits des `DataService` lesen |
| `shutdown` | `bye` | Verbindung geordnet schließen |

Jede Antwort kann stattdessen `error` mit `code`, `message` und `retryable`
enthalten. Der Host meldet den Fehler und schließt die Verbindung; `retryable`
ist eine Information für die Entscheidung über einen neuen Verbindungsversuch.
Andere Fehler bleiben im Rust-API als `StdioError` unterscheidbar, insbesondere
Transportfehler, falsche Korrelation, Registrierung und Observation-Validierung.

Eine vollständige Observation-Antwort sieht beispielsweise so aus:

```json
{"protocol_version":1,"request_id":"stdio-2","message":{"type":"observation","message_id":"temperature-01:sample-1","observation":{"capability_id":"temperature.observe","external_subject":"temperature-01","external_reference":"simulated:temperature-01:sample-1","payload":{"value":25.0,"unit":"Cel"}}}}
```

Der Adapter liefert keine internen Artifact-IDs, Autoritätsklassen oder
Trust-Werte. Diese setzt der Core aus seinen eigenen Regeln und dem Grant.

## Input und allgemeines Reasoning V2

Input besitzt einen eigenen Poll-/ACK-Dialog. Der Core validiert den Input und
speichert Ereignis und Empfangsbeleg atomar, bevor er `ack_input` sendet.
Identische Wiederholungen verwenden dasselbe Artefakt; geänderte Inhalte unter
derselben Nachrichten-ID werden abgewiesen. Inputs verändern weder den
Current-State noch das World Model automatisch.

`reason_v2` verwendet die DTOs `ReasoningInvocationV2` und `ReasoningResponseV2`.
Der Vertrag benennt Verfahren, ausgewählten Kontext, Ziel-Schema und Budgets.
Solver und Regeln benötigen kein Modell, keinen Prompt und keine Tokenangaben.
Der Betreiber erlaubt Verfahren und Ressourcen über `reasoning_limits_v2` im
Grant; modellbasierte Verfahren benötigen zusätzlich die Producer-Klasse `Model`.
V1-DTOs und bestehende Grants bleiben lesbar. Die neuen Nachrichten sind additive
Varianten des stdio-v1-Envelopes und werden nur für passende Capabilities genutzt.

Antworten enthalten den Status `solved`, `no_solution` oder `budget_exhausted`,
Kandidaten und einen Verfahrenstrace. Der Core prüft Korrelation, Schema,
Kontextreferenzen, erlaubte Fähigkeiten und Budgets. Kandidaten erteilen keine
Handlungsbefugnis. Modellherkunft bleibt auch durch weitere Verarbeitung
`Model` mit niedrigem Trust. Der stdio-Host begrenzt die Wartezeit zusätzlich auf
das kleinere Limit aus Verbindung und Reasoning-Auftrag.

Startbefehle, Replay und Grenzen des synchronen Hosts beschreibt das
[Input-/Reasoning-Beispiel](apps/adapter-host/examples/reason-task.md).

## Wiederholung, Commit und Replay

`message_id` ist eine stabile, nicht leere ID mit maximal 128 UTF-8-Bytes und
muss innerhalb einer Adapterinstallation dauerhaft eine konkrete Messung
bezeichnen. Neue Messungen brauchen neue IDs, auch wenn ihr Messwert gleich ist.
Capability, Subject, Referenz, Schema-ID und Payload gehören zur Identität der
ursprünglichen Nachricht und dürfen bei einer Wiederholung nicht wechseln.

Der Adapter hält eine angebotene Messung bis zum `ack` vor und bietet bei
erneutem `poll` dieselbe unbestätigte Messung an. ACKs müssen wiederholbar sein.
Ein produktiver Adapter muss unbestätigte Messungen auch über eigene Neustarts
hinweg vorhalten. Der Simulator erreicht Replay durch seinen unveränderlichen
Satz konfigurierter Samples und beginnt bei jedem Prozessstart am Anfang.
Der Lüfter-Simulator hält seine noch unbestätigten Observations dagegen in
seiner eigenen SQLite-Datenbank vor.

Der Core speichert Observation, Current-State und Wiederholungsbeleg atomar.
Er sendet erst danach `ack`. Die identische Wiederholung liefert dieselbe
Artifact-Version und Logposition, ohne Current-State zurückzusetzen. Dieselbe
ID mit anderer Eingabe führt zu `IdempotencyConflict` und wird nicht bestätigt.
Ein Grant wird auch bei Wiederholungen erneut geprüft.

Bricht die Verbindung nach dem Commit ab, bleibt die Observation im Log,
selbst wenn `poll()` wegen einer ausgebliebenen ACK-Antwort einen Fehler
zurückgibt. Beim Wiederaufbau des World-State wird sie aus `ObservationLog`
eingespielt. Alte Receipt-Replays dürfen nicht nochmals in einen neueren
World-State reduziert werden; die Demo prüft dafür den `ObservationCursor`.

`SqliteArtifactStore::durable_id_generator(node)` reserviert vor Benutzung
einen dauerhaften Namensraum. Damit bleiben neue IDs bei Neustarts und mehreren
Hosts an derselben Datenbank verschieden. Unabhängige Datenbanken haben keine
globale Eindeutigkeitsgarantie. `SequenceIdGenerator::new` allein bleibt für
Tests und flüchtige Demos geeignet. Reservierte Epochen dürfen nicht gelöscht
oder zurückgesetzt werden.

Auch die Materialisierung derselben Freigabe für dieselbe Capability liefert
dieselbe Action. Die bestehende Ausführungswiederholung verwendet dadurch
weiterhin dieselbe Invocation-ID. Bereits vorhandene Actions aus älteren
Stores werden über ihren bisherigen Stream-Key wiedergefunden.

## Requests und Actions

`StdioAdapter` implementiert `RequestAdapter` und `ActAdapter`. Der Host
verwendet ihn mit `RequestService::execute` beziehungsweise
`ExecutionService::execute`. Diese Dienste prüfen Grants, Schemas, Freigaben
und den dauerhaften Invocation-Zustand. Der Transport ersetzt keine dieser
Prüfungen. Die Session serialisiert Dialogschritte auch bei gemeinsamem
Zugriff über die beiden Traits.

Die inneren Verträge sind dieselben `RequestInvocation`/`RequestResponse`
und `ActInvocation`/`ActResponse`, die schon im Rust-API existieren:

```json
{"protocol_version":1,"request_id":"stdio-2","message":{"type":"request","invocation":{"invocation_id":"request:example","capability":{"adapter_id":"demo.fan","capability_id":"temperature.read"},"request_version_id":"request-1-v1","request_schema_id":"arcs.request.temperature.v1","subject":"temperature-01","request_payload":{"device_id":"temperature-01"},"response_schema_id":"arcs.measurement.temperature.v1","max_response_bytes":4096}}}
```

```json
{"protocol_version":1,"request_id":"stdio-2","message":{"type":"request_result","response":{"invocation_id":"request:example","external_reference":"sensor:sample-1","payload":{"value":31.0,"unit":"Cel"}}}}
```

Die verbindungsbezogene `request_id` und die dauerhafte `invocation_id`
haben getrennte Aufgaben. Beide müssen in der jeweiligen Antwort passen.
Bei `execute` enthält `invocation.payload` den vollständigen materialisierten
Action-Payload mit Candidate, Approval, Capability und fachlichem Payload.
Die Antwort enthält `response.result_payload`. Der Core validiert die Antwort
gegen das für die Capability registrierte Ergebnisschema.

ACKs bestätigen Observations. Request-/Action-Antworten verwenden stattdessen
die dauerhafte Invocation-ID zur Wiederholung. Ein idempotenter Executor muss
dieselbe Invocation mit identischer Eingabe auf dasselbe Ergebnis abbilden
und eine widersprüchliche Eingabe unter derselben ID ablehnen. Neue Wire-IDs
beim Reconnect erzeugen keine neue fachliche Invocation.

Transportfehler werden an die vorhandenen Dienste so weitergereicht:

| Fehler | `AdapterCallError` |
| --- | --- |
| Deadline überschritten | `Timeout` |
| Prozessabbruch, unvollständige Antwort oder I/O-Fehler | `Unavailable` |
| Remote-Fehler mit `retryable: true` | `Unavailable` |
| Remote-Fehler mit `retryable: false` | `Rejected` |
| Ungültiges JSON, falsche Version, Antwortart oder Korrelation | `Protocol` |

Damit erhalten die bestehenden Invocation-Dienste die Information für ihre
Recovery-Regeln. Es gibt weiterhin keine automatischen Verbindungs-Retries.
Neue Nachrichten erweitern stdio v1 additiv; ein Adapter bietet nur die
Fähigkeiten an, die er auch tatsächlich implementiert.

## Deterministische Transforms

`TransformAdapter` verarbeitet eine bereits gespeicherte Artefaktversion.
Die Capability hat den Typ `Transform` und benennt ihre akzeptierten und
erzeugten Schema-IDs. Der Aufruf ist eine Datenverarbeitung ohne externe
Wirkungen. Probabilistische Modelle verwenden den separaten Reasoning-Port.

```json
{"protocol_version":1,"request_id":"stdio-2","message":{"type":"transform","invocation":{"invocation_id":"transform:example","capability":{"adapter_id":"example.normalize","capability_id":"number.normalize"},"input_version_id":"input-1-v1","input_schema_id":"arcs.number.raw.v1","input_payload":{"value":310.0,"scale":0.1,"offset":0.0,"unit":"Cel"},"output_schema_id":"arcs.number.normalized.v1","max_output_bytes":4096}}}
```

```json
{"protocol_version":1,"request_id":"stdio-2","message":{"type":"transform_result","response":{"invocation_id":"transform:example","external_reference":"normalized:transform:example","payload":{"value":31.0,"unit":"Cel"}}}}
```

Der Service prüft vor jedem Aufruf und Replay den installierten Endpoint,
die aktuelle Freigabe, die gespeicherte Eingabe und beide Schema-Verträge.
`max_payload_bytes` begrenzt Eingabe und Ausgabe; das Source-Referenzlimit
des Grants gilt für die Rückgabe. Das SDK prüft zusätzlich Routing,
Korrelation und Ausgabelimit. Der Core validiert das komplette Ergebnis
erneut. Der Adapter liefert keine IDs, Trust-Werte, Provenienz oder Freigaben.
Ein reiner Transform-Grant benötigt keinen Ingress-Herkunftskanal. Der Core
setzt die Quelle auf `external`, bei einer Systemkomponente auf `internal`;
der ursprüngliche Datengeber bleibt über Parent und Trust nachvollziehbar.

Die Ergebnisversion erhält die Eingabe als Parent und eine `derived_from`-
Relation. Trust ist höchstens so hoch wie Eingabe und Grant. Modellherkunft
bleibt auch bei einer System-Transformation als Modellherkunft mit niedrigem
Trust erhalten. Transform-Ergebnisse können keine `action`, `approval`,
`verification_report`, `invocation`, `execution_result`, `outcome`,
`reasoning_request`, `reasoning_result` oder `output_request`-Artefakte erzeugen. Kandidaten
bleiben Vorschläge und durchlaufen die separate Verifikation und Freigabe.
Transform-Resultate werden als Ereignisse gespeichert und aktualisieren
keinen World-State-Slot von selbst.

Die dauerhafte Aufrufidentität enthält Capability, Eingabeversion,
Ausgabeschema und Adapterversion. Eine vollständige serialisierte Identität
wird zusätzlich geprüft, bevor ein Ergebnis wiederverwendet wird. Der
Adapter muss dieselbe Eingabe unter derselben ID deterministisch beantworten.
Bei verlorener Antwort darf nach Reconnect mit derselben ID erneut gerechnet
werden. Ein permanenter Fehler ist terminal; temporäre Transportfehler können
über die vorhandenen Recovery-Regeln erneut versucht werden. Es gibt keine
automatischen Retries.

Ergebnis, Relation und erfolgreicher Invocation-State committen atomar.
Transform verwendet das neue interne Schema `arcs.invocation.v2`; das
bestehende `arcs.invocation.v1` bleibt unverändert und wird für die bisherigen
Aufruftypen weiterverwendet. Der gemeinsame Lifecycle liest beide Versionen.
Damit wird kein bereits gebundener V1-Vertrag verändert.

Das Beispiel unter [normalize-number](adapters/normalize-number/README.md)
demonstriert denselben Anschluss mit unterschiedlichen Zahlendaten und Einheiten.

## Kontrollierte Ausgabe

`OutputService::prepare` erstellt einen dauerhaften `arcs.output_request.v1`
mit Ziel-Capability, vollständigem Manifest, Inhaltversion und Ergebnisschema.
Erst eine auf diesen Auftrag bezogene, bestandene Verification und ein
Approval von Human oder System erlauben `deliver`. Der Core prüft die
jeweiligen Autoritätsklassen, internen Herkunftsbelege und die Relationen
`derived_from`, `verifies`, `approves` und `based_on`. Freigaben anderer
Aufträge oder nur des ursprünglichen Inhalts reichen nicht aus.

Der Output-Adapter erhält `OutputInvocation` mit `invocation_id`,
`capability`, `request_version_id`, `approval_version_id`,
`artifact_version_id`, `artifact_schema_id`, optionalem `subject`,
dem eigentlichen `payload`, `result_schema_id` und `max_result_bytes`.
Die Antwort ist ein `output_result` mit `OutputResponse`: korrelierte
`invocation_id`, `external_reference` und `result_payload`.

Die Capability muss `Output` mit `idempotent: true`, passenden
`accepts`-/`emits`-Verträgen und benötigten Permissions deklarieren. Der
Betreiber muss diese explizit freischalten und `producer_class: executor`
zuweisen. Der Input und das Resultat unterliegen dem Payloadlimit, die
Bestätigung dem externen Referenzlimit. Der Core validiert alle Rückgaben;
Steuerartefakte können auch nicht als Output-Resultat eingeschleust werden.

Ausgabe und Action haben getrennte Nachrichten und Ports. Ein Output-Auftrag
umgeht keine Act-Freigabe. Das SDK prüft die Struktur und Korrelation;
die vollständige Approval-Prüfung bleibt beim Core. Der Adapter garantiert
die dauerhafte Idempotenz seiner konkreten Senke.

Der gemeinsame Invocation-Lifecycle verwendet für Output das bereits
vorhandene `kind: output` im unveränderten `arcs.invocation.v1`. Beleg,
Resultat-Relationen und erfolgreicher Zustand committen im Core atomar.
Aktuelle Grants und die Auftragsbindung werden auch beim Replay geprüft.
Nach unterbrochener Übertragung muss dieselbe Invocation-ID wiederverwendet
werden. Der Host führt keine automatischen Wiederholungen oder Reconnects aus.

Ein vollständiger lokaler Nachweis einschließlich Datei-/Journal-Recovery
steht unter [file-output](adapters/file-output/README.md).

## Referenzierte Daten

Eine Observation oder Request-Antwort kann statt großer eingebetteter Daten
eine `DataReference` enthalten. Der allgemeine Beispielvertrag
`arcs.data.reference.v1` liegt im Protokollpaket unter `schemas/` und wird
vom Host ausdrücklich geladen:

```json
{"id":"frame-0001","media_type":"image/png","size_bytes":131073}
```

Die ID ist ein opaker Schlüssel mit 1 bis 96 ASCII-Buchstaben, Ziffern,
Punkten, Unterstrichen oder Bindestrichen; `.` und `..` sind ausgeschlossen.
Sie ist weder Dateipfad noch URL. Identität und Zugriff gelten innerhalb
eines Adapters und einer Capability. Der Medientyp enthält zwei durch `/`
getrennte, kleingeschriebene MIME-Tokens ohne Parameter, insgesamt höchstens
128 Bytes. Eine Referenz darf höchstens 64 MiB bezeichnen, auch leere Objekte
sind erlaubt. Die typisierten APIs prüfen diese zusätzlichen Regeln vor
dem Abruf; das JSON-Schema prüft die Grundstruktur und den Größenbereich.

Für den Abruf braucht der Betreiber-Grant zusätzlich `data.read` und
ausdrückliche Grenzen:

```json
{
  "data_limits": {"max_object_bytes":16777216,"max_chunk_bytes":8192}
}
```

Fehlende oder auf `null` gesetzte `data_limits` sperren den Datenabruf.
Die bisherigen Grants bleiben dadurch unverändert nutzbar. Das Objektlimit
muss 1 bis 67.108.864 Bytes, das Blocklimit 1 bis 8192 Bytes betragen.
Die Capability muss aktiviert und vom Typ Observe, Request oder Transform sein;
Metadaten zu beobachten erteilt noch keine Berechtigung, Inhalte abzurufen.

Ein einzelner Dialogschritt lautet beispielsweise:

```json
{"protocol_version":1,"request_id":"stdio-3","message":{"type":"read_data","request":{"capability":{"adapter_id":"example.files","capability_id":"data.observe"},"reference":{"id":"sample-v1","media_type":"application/octet-stream","size_bytes":3},"offset":0,"max_bytes":8192}}}
```

```json
{"protocol_version":1,"request_id":"stdio-3","message":{"type":"data_chunk","chunk":{"reference_id":"sample-v1","offset":0,"bytes":[0,1,255]}}}
```

`bytes` ist ein JSON-Array von Bytewerten. Auch ein Block mit 8192 Werten
passt in das bestehende Frame-Limit. Referenz-ID, Offset und die exakte
Länge `min(max_bytes, size_bytes - offset)` müssen stimmen. Ein leeres
Objekt wird mit Offset 0 abgefragt und mit einem leeren Block bestätigt.
Für andere Objekte muss der Offset vor dem Ende liegen. Request- und
Protokollfehler behalten die bestehenden Fehlerklassen und Timeouts.

`DataService::copy` prüft die aktuelle Registry und schreibt jeweils nur
einen geprüften Block. `Host::copy_data` stellt das der Anwendung bereit.
Bereits geschriebene Blöcke werden bei einem späteren Fehler nicht
zurückgenommen. Das Exportbeispiel veröffentlicht deshalb eine temporäre
Datei erst nach dem vollständigen Abruf und überschreibt keine Zieldatei.
Die API lädt kein vollständiges Objekt in den Speicher und führt keine
automatischen Retries durch. Die Deadline gilt pro Dialogschritt.

Die Unveränderlichkeit der Bytes ist ein Treibervertrag; dieses erste Format
enthält keine kryptografische Prüfsumme. Die Dateiquelle setzt den Vertrag
mit persistenten Snapshots um und weist veränderte Daten unter derselben ID
ab. Der Core speichert die Referenz als Evidenz, während die Binärdaten bei
der Quelle beziehungsweise dem ausdrücklich gewählten Exportziel bleiben.
Die Blockübertragung bietet noch keinen kontinuierlichen Videostream.

Ein ausführbarer Nachweis mit beliebigen Dateiinhalten steht in der
[Dokumentation der Dateiquelle](adapters/file-data/README.md).

## Simulierten Sensor starten

Aus dem Repository-Verzeichnis:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-demo-host --bin arcs-adapter-demo -- \
  ./target/debug/arcs-simulated-sensor /tmp/arcs-sensor-demo.sqlite \
  adapters/simulated-sensor/config/profile.json
```

Der Host startet den zweiten Prozess selbst. Das Profil beschreibt Feldname,
Skalierung, Offset, Einheit und stabile Sample-IDs. Der Adapter berechnet
`value = raw[field] * scale + offset` und liefert mit dem Beispielprofil 25 °C
und 26 °C. Der Host speichert zwei Observations; der World-State enthält zuletzt
26 °C. Ein erneuter Lauf mit derselben Datenbank lässt die Anzahl bei zwei.
Neue Samples mit neuen IDs werden zusätzlich gespeichert.

Ohne Profilargument verwendet der Sensor sein eingebettetes Beispielprofil;
er ist damit auch unabhängig vom Arbeitsverzeichnis startbar. Sein eigenes
Paket lässt sich mit `cargo build --locked -p arcs-simulated-sensor` bauen.

Die Demo lädt das Temperatur-Schema und den Grant aus den mitgelieferten,
beim Bauen eingebetteten Beispieldateien. Das optionale Profilargument wird
dem Adapter unverändert übergeben. Diese Demo ist kein allgemeiner
Konfigurationslader. Der Core enthält weder Temperaturwissen noch ein
Geräteprofil. Weitere Treiber und Profile, Rohdatenverträge, semantische
Erkennung sind weitere Ausbauschritte. Referenzierte Binärdaten können
inzwischen mit `read_data` blockweise übertragen werden.

## Vollständigen Lüfter-Zyklus starten

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-demo-host --bin arcs-fan-demo -- \
  ./target/debug/arcs-simulated-fan \
  /tmp/arcs-fan-core.sqlite /tmp/arcs-fan-adapter.sqlite \
  adapters/simulated-fan/config/profile.json
```

Der Host startet den Adapter selbst. Das Profil enthält Geräte-IDs,
Temperatur-Normalisierung und die Demo-Regel: über 28 °C einen eingeschalteten
Lüfter vorschlagen, automatische Freigabe höchstens bis 80 °C erlauben.
Mit den Standarddaten entsteht eine Messung von **31 °C**, der simulierte
Lüfter ist anschließend eingeschaltet und sein Schaltzähler steht auf **1**.

Der Ablauf ist vollständig protokolliert:

1. Eine stabile Cycle-ID bindet Profil und gewünschte Freigabeentscheidung an
   ein persistiertes Input-Artefakt.
2. `temperature.read` beschafft gezielt den Messwert. Der Adapter veröffentlicht
   die Messung zusätzlich als Observation; nur deren Commit verändert den
   World-State.
3. Die Host-Regel erzeugt über `decision::proposal::ProposalService` einen
   deterministischen Candidate mit Regel-ID, Evidenz und erforderlicher
   Capability. Der Dienst prüft Schema, vorhandene Evidenz und freigeschaltete
   Capabilities und speichert den Vorschlag idempotent. Ein solcher Vorschlag
   hat keine Aktionsautorität und keine erfundene Modellspur.
4. Ein separater Verifier prüft Zielgerät, Einheit, Temperaturbereich und
   Evidenz. Die Freigabe erteilt anschließend ausdrücklich die System-Policy
   `demo.fan.simulation-policy`; sie wird nicht als menschliche Freigabe
   ausgegeben. Die Policy gilt ausschließlich für diesen Simulator.
5. Erst danach materialisiert der Host die Action und führt `fan.set` aus.
6. `fan.read` fragt den tatsächlichen Simulatorzustand ab. Ausführung und
   Rückmeldung werden als Outcome bewertet und über Relationen verknüpft.
   Persistierte Observations aktualisieren den World-State.

Die Ausgabe enthält Status, Messung, Lüfterzustand, World-State und die
Artifact-Versionen des Audit-Trails. `actuation_count` zählt tatsächliche
Zustandswechsel, nicht bloß empfangene Aufträge.

Ein erneuter Lauf verwendet standardmäßig dieselbe Cycle-ID `cycle-1` und
liefert dieselben Artefakte und Ergebnisse, ohne neu zu schalten. Eine neue
gezielte Abfrage beginnt mit einer neuen Cycle-ID als optionalem fünften
Argument, beispielsweise `cycle-2`. Dieselbe Cycle-ID mit geändertem Profil
oder anderer Freigabeentscheidung wird als Identitätskonflikt abgewiesen.

Für eine explizite Ablehnung kann am Ende `--reject` angegeben werden. Bei
einer frischen Datenbank bleibt der Lüfter dann aus. Unterhalb oder genau auf
der Einschaltschwelle entsteht kein Vorschlag; außerhalb des konfigurierten
Freigabebereichs scheitert Verification. Diese Einschalt-Demo schaltet einen
bereits laufenden Lüfter bei einer Ablehnung oder niedrigeren Temperatur
nicht automatisch wieder aus.

Core und Adapter besitzen getrennte Datenbanken. Der Simulator schreibt
Gerätezustand, Invocation-Ergebnis und ausstehende Observation in **einer
Transaktion**. Auch bei einer verlorenen Execution-Antwort bleibt deshalb
eine Wiederholung mit derselben Invocation-ID ohne zweite Wirkung.
Beide Datenbanken müssen für den Replay erhalten bleiben.

Diese atomare Garantie gilt für den simulierten SQLite-Gerätezustand. Ein
Treiber für echte Hardware muss die Idempotenz und Recovery des jeweiligen
Geräts implementieren. Temperaturregeln, Geräteprofile und Demo-Schemas liegen
in `apps/demo-host` beziehungsweise `adapters/*/config` und den jeweiligen
Adapter-Bibliotheken; der Core enthält keine Lüfter- oder Temperaturregel.
Der Lüfter-Adapter baut mit `cargo build --locked -p arcs-simulated-fan`
ohne Core-Abhängigkeit. Der Host registriert seine veröffentlichten Schemas;
der Adapter prüft Eingaben anhand seiner eigenen typisierten Geräteverträge.

```bash
cargo test --locked --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features
```

## Mehrere Installationen jeder Rolle

Alle sieben Rollen können gleichzeitig durch mehrere Installationen angeboten
werden. Das gilt für Geräte ebenso wie für Softwaredienste und unterschiedliche
Reasoning-Verfahren. Eine Installation darf mehrere Rollen anbieten.

Die `adapter_id` im Manifest identifiziert genau eine Installation; lokale
Capability-Namen dürfen zwischen Installationen identisch sein. Aufrufe
benennen stets beide Teile der `CapabilityRef`. Jeder Anschluss erhält einen
eigenen Betreiber-Grant; Journale zustandsbehafteter Adapter müssen an die
Installation gebunden sein. Ein Reasoner erlangt durch seine Auswahl keine
Ausführungsrechte.

`CapabilityContract::role()` stellt die Rolle als `AdapterRole` bereit.
`AdapterRegistry::catalog()` und `capabilities(role)` zeigen freigeschaltete
Fähigkeiten. Der allgemeine Host bietet eine externe Sicht mit
`adapter_catalog()` und `capabilities(role)`; `--list-adapters` gibt diese nach
dem Handshake ohne Polling aus. Rollen, Schemas und erlaubte V2-Verfahren
helfen der Anwendung bei der ausdrücklichen Auswahl. Eine Katalogbeschreibung
ist weder ein Health-Check noch ein Autorisierungstoken.

Der [Prozessnachweis](apps/adapter-host/examples/multiple-adapters.md) deckt
alle Rollen mehrfach ab. Die Anschlüsse werden aktuell beim Host-Start
konfiguriert; dynamisches Hotplug und parallele Orchestrierung folgen separat.

## Kontext von Sensoraufnahmen

Das geräteunabhängige `CaptureContext` beschreibt Aufnahme-ID, angebotene
Quell-Capability, Sensor-ID, Aufnahmezeit und Uhrdomäne, optionalen lokalen
Zustandsbezug, Koordinatenrahmen und Qualitätsangaben. DTO und Schema liegen im
Protokollpaket; konkrete Messwerte und Kamerageometrie bleiben bei den Treibern.
Fehlender Zustandsbezug und fehlende Zeitunsicherheit werden als unbekannt
behandelt. `nominal_age_ms` vergleicht ausschließlich dieselbe Uhrdomäne und
behauptet weder Synchronisation noch kalibrierte Genauigkeit.

Der äußere Core-Envelope bleibt für Producer und Trust maßgeblich. Die
Quellangabe innerhalb eines Payloads muss an diesen Envelope und die konfigurierte
Capability gebunden werden. Sie erzeugt keine zusätzliche Datenfreigabe.
Qualitätsangaben wie `simulated` oder `idealized_simulation` beschreiben das
Messmodell und sind keine Autoritätsklasse.

Der [externe Kamera-Nachweis](../drone-sim/adapters/simulated-drone/CAMERA.md) verwendet diesen Kontext
mit einem unveränderlichen Bildsnapshot und einer zugeordneten Pose. JSON-Schema
prüft die Form; der Treiber und die fachliche Anwendung prüfen zusätzlich die
Beziehungen zwischen Zustand, Bildreferenz, Kalibrierung und Quelle.

## Binäreingaben für Reasoning V2

Die additive stdio-Erweiterung verwendet vor `reason_v2`:

1. `begin_input_data { invocation_id, target, inputs }` →
   `input_data_ready { invocation_id }`.
2. Für jedes Objekt `input_data_chunk { invocation_id, name, offset, bytes }` →
   `input_data_stored { invocation_id, name, next_offset }`.
3. `reason_v2` enthält dieselben Deskriptoren im optionalen Feld `input_data`.
   Jeder Deskriptor besteht aus `selection` (Name, Quell-Capability,
   Quellversion, Referenz-Pointer) und der exakt ausgewählten `reference`.

Blöcke enthalten höchstens 8192 Bytes und bleiben unter der JSON-Lines-
Framegrenze. SDK-Puffer sind auf einen ausstehenden Auftrag, acht Objekte und
insgesamt 16 MiB beschränkt. Exakte Offsets, erwartete Gesamtlänge und
Invocation-/Capability-Bindung sind Pflicht. Erst vollständig übertragene,
im Kontext referenzierte Daten erreichen `Adapter::reason_with_data`.
Ein fehlerhafter Dialog verwirft den Puffer; eine erfolgreiche Übergabe
verbraucht ihn einmal. Der Host prüft ACK-Identitäten und beendet einen Prozess
nach Fehler oder Timeout. Upload und Reasoning teilen das Runtimebudget.

Die Betreiberrechte sind getrennt: Quelle `data.read`/`data_limits`, Empfänger
`data.receive`/`input_data_limits`. Die tatsächliche Herkunft stammt aus dem
Core-Envelope. Der [Reasoning-Vertrag](docs/REASONING.md#explizite-binäreingaben)
beschreibt Kontextbindung, Replay und Grenzen. Alte Dialoge ohne Binäreingaben
ändern sich nicht. Transform-Binäreingaben sind noch nicht implementiert.
