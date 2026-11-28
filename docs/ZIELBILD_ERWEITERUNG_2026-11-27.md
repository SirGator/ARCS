# ARCS – Zielbilderweiterung: Unified Runtime, Datenschutz, formale Methoden, Betrieb und Unternehmensfähigkeit

**Spezifikation v3.1-Erweiterung · 27. November 2026**

> Diese Erweiterung ergänzt das verbindliche Zielbild aus
> [ARCHITECTURE.md](ARCHITECTURE.md) (v3.0) sowie die Abschnitte 3–15 von
> [TECHNICAL_ARCHITECTURE_V3.md](TECHNICAL_ARCHITECTURE_V3.md).
> Sie beschreibt den **angestrebten Zustand**, keine bereits abgenommenen
> Implementierungen. Bestehende Anforderungen, IDs und historische
> Referenzstände bleiben unverändert gültig.
>
> Arbeitsgrundlage für die Umsetzung:
> [Implementierungsplan v2.0](IMPLEMENTATION_PLAN.md).

## Einordnung, Geltung und ID-Regeln

Die Erweiterung ist eine Ergänzung, kein Ersatz. Konflikte werden nach
Geltungsbereich aufgelöst:

| Frage | Auflösung |
| --- | --- |
| Grundsatz | Zielbild und Systemverträge der Erweiterung gelten zusätzlich. Bestehende v2-/v3-Anforderungen bleiben unverändert in Kraft. |
| Bestehende Implementierung | Vor jeder Umsetzung sind Goal-Runtime, Task-Queues, Journale, Ressourcenreservierungen, Memory-Scopes, Admission-Regeln und Adapterverträge zu prüfen und zu **erweitern**, nicht zu duplizieren. |
| Core und Manager | Die sechs Manager (Input, Observe, Request, Reason, Act, Output) und Transform als siebte Adapterrolle bleiben unverändert. Die Erweiterung führt **keine** siebte Managerinstanz und **keine** zusätzliche oberste Kontrollinstanz ein. |
| Scheduler | Der Scheduler bleibt ein Ressourcen- und Zustandsdienst. Er ist keine semantische Befehlsinstanz und keine neue Autoritätsquelle. |
| ID-Namensraum | Neue Anforderungs-IDs (`CTX-*`, `PRIV-*`, `FORM-*`, `OPS-*`, `BIZ-*`) gelten innerhalb dieser Erweiterung. Bestehende Aufgaben werden weder umnummeriert noch als erledigt markiert. |
| Namensraumkonflikt `OPS-*` | `OPS-01`–`OPS-06` dieser Erweiterung (§21) sind **nicht** das Paket `OPS-01` aus v3 §12 (Betrieb und Skalierung). Verweise schreiben immer „Erw. §21 OPS-xx“ bzw. „v3 §12 OPS-01“. |
| TEST-IDs | `TEST-06`–`TEST-12` schließen an die Testreihe aus v3 §13 an. Die Systemtests `TEST-01`–`TEST-03` des Implementierungsplans v2.0 (§3) bleiben davon unberührt. |
| Prioritäten | `P0` = Architektur- und Sicherheitsgrundlagen, `P1` = darauf aufbauende Funktionsnachweise, `P2` = optionale Forschung. |

### Codestand, auf den sich diese Erweiterung bezieht

Die Zuordnungen in den folgenden Abschnitten verweisen auf den lokalen
Implementierungsstand in `ARCS/crates` und `ARCS/apps`, nicht auf den
historischen Referenzstand aus v3 §2. Genannte Dateien sind Ausgangspunkt
für die Erweiterung, nicht Beleg für deren Erfüllung.

## 17. Zusätzliche verbindliche Architekturprinzipien

Die folgenden fünf Prinzipien ergänzen die acht Prinzipien aus v3 §1. Sie sind
verbindlich und werden in [ARCHITECTURE.md](ARCHITECTURE.md) §1 sowie in
[TECHNICAL_ARCHITECTURE_V3.md](TECHNICAL_ARCHITECTURE_V3.md) §1 mitvermerkt.

9. **Gemeinsame Infrastruktur, unabhängige Arbeitszustände:** Ein Core und
   logisch gemeinsames Memory; keine vollständige Agenteninstanz pro Aufgabe.
10. **Zweckgebundene Informationsflüsse:** Zugriff auf Wissen und Übertragung
    von Wissen sind eigenständige, überprüfbare Berechtigungen.
11. **Formale Methoden:** Lean ist als dauerhaft verfügbare Reasoning- und
    Verifikationsfähigkeit integriert; die Engine bleibt isoliert, Werkzeuge
    bleiben externe Komponenten.
12. **Kontinuierliche, nachvollziehbare Arbeit:** ARCS bearbeitet laufende
    Ziele, reagiert auf Ereignisse und gibt autorisierte Auskunft über eigene
    Arbeitszustände.
13. **Organisationsfähigkeit als Anwendung des allgemeinen Systems:** Rollen,
    Ziele, Budgets und betriebliche Prozesse werden durch Artefakte, Policies
    und Capabilities ausgedrückt, nicht als fest verdrahtete Unternehmenslogik
    im Core.

Die Zielrichtung bleibt: eine **universelle, dauerhaft verfügbare ARCS-Instanz**,
die persönliche Assistenz, Unternehmensarbeit, Forschung und technische
Aufgaben gleichzeitig ausführen kann – als zusammenhängendes logisches System
mit sechs spezialisierten Managern, einem gemeinsamen Artifact-Netz,
austauschbaren Reasonern und kontrollierter Autonomie. Unterschiedliche
Tätigkeiten sind weder vollständig isolierte Agenten noch ein einziger
unstrukturierter Modellkontext.

> **Umsetzungsstand 28. November 2026:** Persistente Arbeitskontexte sind
> umgesetzt und nachgewiesen. Jede bearbeitete Aufgabe erhält einen
> leichtgewichtigen Kontext im Core, der Ziel, Task, Planrevision,
> Artefaktversionen, Authority-Scope, Memory-Sicht, Priorität, Budget und
> Operationen bindet. Zustandsübergänge sind an den gespeicherten Stand
> gebunden, das Ergebnis wird als Operation gebunden und ein begrenzter
> Statusbericht nennt Zustand und Bindungen ohne Nutzdaten. Damit sind `CTX-01`,
> `CTX-03` und `CTX-05` teilweise erfüllt, `CTX-09` in der Grundlage. Die
> folgenden Abschnitte beschreiben den weiteren Bedarf unverändert fort.
> Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_ARBEITSKONTEXTE.md).

## 18. Unified Multi-Context Runtime – eine Instanz, mehrere Tätigkeiten

Ergänzt v3 §5, §6, §7 und §8. ARCS muss persönliche Interaktion und
langfristige Hintergrundaufgaben nebenläufig bearbeiten können. Abhängige
Schritte bleiben seriell geordnet; unabhängige Schritte dürfen parallel laufen.
Der gemeinsame Core muss autorisiert Auskunft über laufende Tätigkeiten,
Ergebnisse, Fehler und Freigaben geben können, ohne vollständige separate
Agenten zu starten oder auf den Abschluss einer Hintergrundaufgabe zu warten.

### 18.1. Arbeitskontexte statt Agent Spawning

| ID | Prio | Anforderung |
| --- | --- | --- |
| CTX-01 | P0 | Ein leichter, persistenter Arbeitskontext referenziert Ziel, Task, Planrevision, Artifact-Versionen, Arbeitszustand, Rollen-/Authority-Scope, zulässige Memory-Sicht, Priorität, Budget und laufende Operationen. |
| CTX-02 | P0 | Core-Dienste, Adapterregistrierung, Verifier, Artifact-Store und geeignete Reasoner werden gemeinsam genutzt. Modellgewichte dürfen gemeinsam geladen sein; Anfragekontexte, Sessions und vertrauliche KV-Caches dürfen nicht unkontrolliert vermischt werden. |
| CTX-03 | P0 | Kontextwechsel sichern explizite Zustände und Referenzen an definierten sicheren Übergängen. Wartende Aufgaben benötigen keine aktive Modellinstanz. Laufende externe Operationen bleiben ihrem ursprünglichen Task zugeordnet, auch wenn ARCS inzwischen eine andere Tätigkeit bearbeitet. |
| CTX-04 | P0 | Scheduling unterstützt asynchrone Wartezustände, priorisierte interaktive Anfragen, Fairness, begrenzte Parallelität, Backpressure und Budgets. Präemption ist nur an sicheren Grenzen zulässig. |
| CTX-05 | P0 | Resultate, Events, Toolantworten und Freigaben müssen eindeutig an Task, Kontext, Planrevision und Operation gebunden sein. Kein globaler veränderlicher „aktueller Kontext“. |
| CTX-06 | P1 | Explizites Agent Spawning bleibt optional für tatsächlich benötigte Isolation, externe Delegation oder unabhängige Prozesse. |
| CTX-07 | P1 | Ressourcenmessung pro Task und gemeinsamem Dienst: RAM, VRAM, Kontexttokens, Modellaufrufe, Latenz, Durchsatz und Kosten. |

**CTX-01 – Arbeitskontext.**
*Bezug zum Ist-Stand:* Die Task-Queue liegt als persistenter Journal-Store in
`apps/adapter-host/src/task_runtime/journal/queue.rs` (`TaskQueueQuery`,
`TaskQueueScope`, `TaskWait`), die Planrevisionen als versionierte
`arcs.plan.v1`–`arcs.plan.v4` in `crates/arcs-core/schemas/`, die
Warte- und Wiederaufnahmezustände in `journal/suspension.rs`,
`journal/scheduling.rs` und `journal/recurrence.rs`. Budget-, Ressourcen- und
Prioritätsangaben existieren bereits getrennt (`apps/adapter-host/src/resource_policy.rs`,
`crates/arcs-core/src/runtime/reservations.rs`, `journal/dependencies.rs`).
*Erweiterungsbedarf:* Ein **eigener, schlanker Arbeitskontext** ist bisher
nicht als einheitliches Objekt vorhanden. Die Anforderungen sind zu einem
persistenten Kontextartefakt zusammenzuführen, das die genannten Felder
*bündelt* und dabei auf die bestehenden Einzelverträge verweist, statt diese zu
duplizieren. Authority-Scope und zulässige Memory-Sicht sind noch nicht Teil
eines Kontextobjekts (siehe CTX-08, CTX-10, PRIV-02).

**CTX-02 – Gemeinsame Infrastruktur.**
*Bezug zum Ist-Stand:* Adapterregistrierung und Capability-Katalog liegen in
`crates/arcs-core/src/adapters/registration/registry.rs`, die Berechtigungen
in `registration/grant.rs` (`AdapterGrant`), der Artifact-Store in
`crates/arcs-core/src/store/`, die Verifier in
`crates/arcs-core/src/decision/verification/`. Das gemeinsame Gewicht:
`crates/arcs-core/src/core/artifact/types.rs` kennt bereits `Actor`,
`ActorType`, `SourceClass` und `TrustLevel`.
*Erweiterungsbedarf:* Der Nachweis, dass zwei Tätigkeiten denselben Core,
denselben Store und dieselben Adapter teilen, muss als Vertrag geführt werden:
Adapter-Sessions sind heute pro Adapter exklusiv (`runtime/sessions.rs`), was
bei parallelen Tätigkeiten mit gemeinsamem Adapter kollidieren kann. Für
CTX-02 ist zu klären, welche Adapter parallelisierbar sind und wie
Session-Slots pro Tätigkeit zugeordnet werden, ohne vertrauliche Kontexte zu
vermischen (KV-Cache-Isolation).

**CTX-03 – Sichere Kontextwechsel.**
*Bezug zum Ist-Stand:* Das Journal (`apps/adapter-host/src/task_runtime/journal/`)
hält Zustandsübergänge fest; `recovery.rs` und `suspension.rs` behandeln
Wiederaufnahme; `crates/arcs-core/src/runtime/invocation/service.rs` erzeugt
deterministische `invocation_id` über `deterministic_invocation_id`.
*Erweiterungsbedarf:* Der Kontextwechsel braucht eine **explizite
Sicherungsgrenze** (Checkpoint), an der Zustand, Referenzen und offene
Operationen gemeinsam eingefroren werden. Bereits laufende Modellaufrufe
oder externe Wirkungen dürfen dabei nicht unterbrochen werden (CTX-04).
„Wartende Aufgaben brauchen keine Modellinstanz“ ist heute strukturell
erfüllt, weil Wartezustände im Journal liegen – es ist als Vertrag zu
formalisieren.

**CTX-04 – Scheduling.**
*Bezug zum Ist-Stand:* Prioritäten, Fristen, Wartezustände, Seriensteuerung
und Ressourcenpools existieren in `task_runtime/journal/dependencies.rs`,
`journal/deadlines.rs`, `journal/scheduling.rs`,
`task_runtime/scheduler/{recurrence,series_control}.rs`,
`apps/adapter-host/src/runtime/resources.rs` und `resource_policy.rs`.
*Erweiterungsbedarf:* Fairness, Backpressure, Budgets und **Prioritätsklassen
für interaktive Anfragen gegenüber Hintergrundarbeit** sind als ein
gemeinsames Scheduling- und Abrechnungsmodell zu führen. Der Scheduler bleibt
Ressourcen- und Zustandsdienst; er darf keine fachlichen Entscheidungen
treffen.

**CTX-05 – Eindeutige Bindung von Ergebnissen.**
*Bezug zum Ist-Stand:* `InvocationSpec` (`runtime/invocation/service.rs`) bindet
`invocation_id`, `kind`, `capability`, `input_version` und `input_fingerprint`;
`Provenance` (`core/artifact/types.rs`) bindet Eltern, Regeln, Modelle und
Transform; `Arcs`-Artefakte sind unveränderlich und versioniert; der
Action-Service (`action/execution/service.rs`) registriert durable Invocation
und Fence vor dem Aufruf.
*Erweiterungsbedarf:* Die Bindung an **Task, Kontext, Planrevision und
Operation** ist über alle Ergebnispfade (Results, Events, Toolantworten,
Freigaben) zu vervollständigen. `InvocationKind` kennt heute
Request/Transform/Reasoning/Execution/Output, aber keinen Arbeitskontext.
Ein global veränderlicher „aktueller Kontext“ existiert zwar nicht, muss aber
als Negativregel festgehalten und getestet werden.

**CTX-06 – Optionales Agent Spawning.**
*Bezug zum Ist-Stand:* Es existiert kein allgemeiner Agent-Spawn-Mechanismus;
`crates/arcs-core/src/runtime/agent_cycle/` beschreibt den Zyklus innerhalb
eines Prozesses.
*Erweiterungsbedarf:* Ein Spawn-Mechanismus ist **nicht** zu implementieren,
solange er nicht für externe Delegation oder echte Isolation benötigt wird.
Die Anforderung dient als Abgrenzung.

**CTX-07 – Ressourcenmessung.**
*Bezug zum Ist-Stand:* `resource_policy.rs` definiert Betreiber-Pools
(`resource_pools`) und Anforderungen (`reasoning_resources`);
`runtime/resources.rs` führt die Belegung; `runtime/reservations.rs`
reserviert exklusive Ressourcen revisiongenau.
*Erweiterungsbedarf:* Messwerte fehlen für **VRAM, Kontexttokens, Kosten,
Durchsatz und Wiederaufnahmekosten** je Task und je gemeinsamem Dienst.
Erforderlich ist eine Vergleichsbaseline gegen getrennte Vollinstanzen
(TEST-12).

### 18.2. Gemeinsames Artifact-Memory mit kontrollierten Sichten

| ID | Prio | Anforderung |
| --- | --- | --- |
| CTX-08 | P0 | Gemeinsames logisches Artifact-Netz als Wahrheitsquelle. Wissenselemente und Beziehungen erhalten bzw. referenzieren Herkunft, Eigentümer, Geltungsbereich, zulässige Zwecke und Zugriffspolitik. |
| CTX-09 | P0 | Persönliche Assistenz ist eine Interaktionsform desselben Systems. Autorisierte Abfragen greifen direkt auf persistente Arbeitsstände, Belege und Ergebnisse zu; Berichte unterscheiden belegte Ergebnisse, laufende Arbeit, Schätzungen, blockierte Aufgaben und offene Entscheidungen. |
| CTX-10 | P0 | Bereichsübergreifende Zusammenarbeit erfolgt über explizite, minimal erforderliche und überprüfte Informationsfreigaben. Zugriff erfolgt nach tatsächlich geltender Autorität, nicht allein aufgrund der Rolle „Assistent“. |
| CTX-11 | P1 | Physisch getrennte Speicher, Prozesse oder Nodes bleiben für besonders sensible Daten möglich, erreichbar über eine einheitliche logische Memory-Schnittstelle. |

**CTX-08 – Herkunft, Eigentümer, Geltungsbereich, Zweck, Zugriffspolitik.**
*Bezug zum Ist-Stand:* Das Artifact-Modell (`core/artifact/types.rs`) führt
`provenance` (Eltern, Regeln, Modelle, Transform), `source`, `trust` und
`tags`; Relationen existieren typisiert in `crates/arcs-core/src/store/relations/`
(`derived_from`, `supports`, `contradicts`, `supersedes`, `part_of_goal`,
`solved_by`).
*Erweiterungsbedarf:* **Eigentümer, Geltungsbereich, zulässiger Zweck und
Zugriffspolitik fehlen als Artefaktattribute.** Diese sind in das Artefaktmodell
und die Schema-Registry (`core/schema/registry.rs`, `crates/arcs-core/schemas/`)
zu ergänzen, ohne das unveränderliche Versionierungsmodell aufzugeben.

**CTX-09 – Assistenz als Interaktionsform.**
*Bezug zum Ist-Stand:* Das Journal liefert bereits lesbare Zustandssichten
(`journal/queries.rs`, `journal/queue.rs`), der Store hält Belege.
*Erweiterungsbedarf:* Der Bericht muss **epistemisch getrennte Abschnitte**
führen: belegte Ergebnisse, laufende Arbeit, Schätzungen, blockierte Aufgaben,
offene Entscheidungen. Als Grundlage dienen die vorhandenen epistemischen
Bewertungen (`crates/arcs-core/src/memory/records.rs`,
`crates/arcs-core/src/reasoning/context/epistemic.rs`). Fortschrittswerte ohne
Beleg sind unzulässig.

**CTX-10 – Minimale Informationsfreigaben.**
*Bezug zum Ist-Stand:* Zugriffsrechte werden über `AdapterGrant` und
`DataLimits`/`InputDataLimits` (`registration/grant.rs`) sowie über
`MemoryPolicy`/`MemoryScope` (`reasoning/context.rs`) beschrieben.
*Erweiterungsbedarf:* Der Autoritätsentscheid muss **pro Anfrage** aus der
geltenden Authority des Anfragenden abgeleitet werden. Die Rollenangabe
„Assistent“ darf allein keine Leserechte erzeugen; ein Adapter, der nur
Verfügbarkeit statt Termindetails liefern kann, braucht dafür eine explizite,
prüfbare Sicht.

**CTX-11 – Physische Trennung bei logischer Einheit.**
*Bezug zum Ist-Stand:* Der Store ist über `crates/arcs-core/src/store/database/`
gekapselt; Adapter laufen als getrennte Prozesse mit eigener, geerbter
Prozessumgebung (`apps/adapter-host/src/process_environment.rs`).
*Erweiterungsbedarf:* Für einen zweiten, sensibleren Speicher ist ein
logisch einheitlicher, physisch getrennter Zugang vorzusehen. Das ist keine
Voraussetzung für CTX-08/09/10, aber die Memory-Schnittstelle darf den
Speicherort nicht hart kodieren.

## 19. Datenschutz und kontrollierte Informationsflüsse

Ergänzt v3 §6.5 (MEM-08) sowie §10.1 (SEC-01 bis SEC-10). Reasoning-Qualität
und Vertraulichkeit sollen gemeinsam optimiert werden. Nicht jeder Reasoner,
jedes Tool oder jeder Benutzer darf alle Daten sehen, die ARCS verwaltet.

> **Umsetzungsstand 28. November 2026:** Schutzstufen, erzwungene Ableitung
> und die Default-Deny-Prüfung an Abruf und Kontextbildung sind umgesetzt
> und nachgewiesen. Damit sind `PRIV-01`, `PRIV-02` und `PRIV-03` teilweise
> erfüllt; `PRIV-07` ist in der Datenklassenprüfung umgesetzt. Die folgenden
> Abschnitte beschreiben den weiteren Bedarf unverändert fort. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_SCHUTZSTUFEN.md).
>
> **Umsetzungsstand 28. November 2026 (zweiter Teil):** Geheimnisse liegen
> in einem Tresor außerhalb des Artefaktnetzes. Der Core vermittelt sie
> über auditierbare Freigaben, die an Zweck, Ressource, Ablaufzeit und
> Nutzungszahl gebunden sind; der gespeicherte Auftrag behält nur die
> Referenz, und erst der ausgehende Auftrag trägt den Wert. Eine
> Egress-Grenze im Adapter-Grant legt fest, wohin Daten einer Installation
> gelangen dürfen; die Prüfung erfolgt vor dem Versand, sodass ein Auftrag
> seine Reichweite nicht erweitern kann. Damit sind `PRIV-04` in der
> Grundlage und `PRIV-05` für die vom Core kontrollierten Grenzen umgesetzt.
> Offen bleiben `PRIV-06`, `PRIV-08` bis `PRIV-11` und die Sandbox des
> Kindprozesses. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_VERTRAULICHE_FAEHIGKEITEN.md).
>
> **Umsetzungsstand 28. November 2026 (dritter Teil):** Ein vertraulicher
> Sachverhalt wird lokal in Variablen zerlegt, extern nur als Struktur
> bearbeitet und lokal rekonstruiert. Die Abstraktion ist eine ausdrücklich
> freigegebene Herabstufung mit Herkunftsbeleg und Prüfung; ein sichtbares
> Feld, das den sensiblen Wert wiederholt, bricht sie ab. Das abstrakte
> Problem gehört genau einem freigegebenen Reasoner, und die Bindung der
> Platzhalter an die Werte bleibt im Core. Ergänzend prüft die v2-Kante des
> Reasonings die Schutzstufe jedes Kontextartefakts selbst, statt dem
> Aufrufer zu vertrauen. Damit sind `PRIV-08` in der Grundlage, `PRIV-09`
> für die Nutzlast und `PRIV-10` für die Rekonstruktion umgesetzt, `PRIV-02`
> an beiden Reasoning-Kanten nachgezogen. Offen bleiben `PRIV-06`,
> `PRIV-09` für erzeugten Code, `PRIV-10` für die formale Prüfung der
> Anwendbarkeit, `PRIV-11` sowie die Messung der Baselines. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_ABSTRAKTION.md).

### 19.1. Informationsflusskontrolle und Authority

| ID | Prio | Anforderung |
| --- | --- | --- |
| PRIV-01 | P0 | Artefakte und abgeleitete Inhalte tragen explizite Sicherheitsattribute und Herkunftsbezüge. Vorgeschlagenes Standardprofil: Public, Internal, Confidential, Restricted, Secret (S0–S4); tatsächlich gelten konfigurierbare Policies. Eine Stufe allein genügt nicht zur Autorisierung. |
| PRIV-02 | P0 | Default deny für unbekannte oder nicht autorisierte Informationsflüsse. Memory-Retrieval, Kontextbildung, Transform, Request, Reason, Act und Output erzwingen die jeweils gültigen Leserechte und Transferregeln. Prüfung vor Offenlegung. |
| PRIV-03 | P0 | Abgeleitete Artefakte, Zusammenfassungen, Embeddings, Graphkanten, Caches und Logs erben die relevanten Schutzanforderungen ihrer Quellen konservativ. Herabstufung erfordert eine ausdrücklich autorisierte, nachvollziehbare Transformation samt Prüfung. |
| PRIV-04 | P0 | Secrets gelangen nicht in LLM-Kontexte oder gewöhnliche Logs. Ein Secret-Broker nutzt kurzlebige, zweck- und ressourcenbegrenzte Berechtigungen. Agenten erhalten autorisierte Fähigkeiten statt frei lesbarer Schlüssel. |
| PRIV-05 | P0 | Sandbox, Dateisystemrechte, Datenbankzugriffe und Netzwerk-Egress erzwingen Policies außerhalb der Modelle. Unvertrauenswürdige Dokumente, Toolantworten und Prompt-Injection können keine zusätzlichen Rechte erzeugen. |
| PRIV-06 | P1 | Nachweisbare Löschung, Aufbewahrungsfristen, Zugriffsaudit und Datenherkunft unter Berücksichtigung von Indizes, Backups und abgeleiteten Daten. |
| PRIV-07 | P0 | Modellrouting prüft zuerst Zulässigkeit (Datenklasse, Zweck, Empfänger, Betreiber, Verarbeitung, Vertrags-/Policy-Vorgaben), erst danach Qualität, Kosten und Latenz. |
| PRIV-08 | P1 | Lokale Klassifikation, Pseudonymisierung, Aggregation, Generalisierung, strukturierte Problemabstraktion und minimale Kontextprojektion erlauben den Einsatz starker externer Modelle ohne unnötige Offenlegung. |
| PRIV-09 | P1 | Symbolisches Reasoning: Ein externer Reasoner erarbeitet anhand abstrakter Variablen, Schnittstellen und Regeln einen Algorithmus, Plan oder Beweisvorschlag; tatsächliche Werte werden lokal eingesetzt. Generierter Code wird vor Ausführung geprüft und isoliert. |
| PRIV-10 | P1 | Lokale Rekonstruktion und unabhängige Verification stellen sicher, dass eine abstrahierte Lösung auf das reale Problem anwendbar ist. |
| PRIV-11 | P2 | Confidential Computing mit überprüfbarer Attestation sowie Differential Privacy, sichere Mehrparteienberechnung und homomorphe Verschlüsselung für geeignete Spezialfälle evaluieren. |

**PRIV-01 – Sicherheitsattribute.**
*Bezug zum Ist-Stand:* `Artifact` (`core/artifact/types.rs`) führt `trust`
(`TrustLevel`, `source_class`), `source`, `provenance` und `tags`. Eine
**Klassifikationsstufe, ein Eigentümer, ein Zweck, ein Aufbewahrungs- oder
Empfängerbezug existiert nicht**.
*Erweiterungsbedarf:* Ein Sicherheitsprofil-Feld (S0–S4 als Default-Profil)
plus Policy-Bezug sind in Artefaktmodell, Schema-Registry und Migration
einzuführen. Wichtig: `TrustLevel` und Klassifikation sind **verschiedene
Dimensionen** – Herkunftsvertrauen und Datenempfindlichkeit dürfen nicht
vermischt werden. Das bestehende `Trust` bleibt unverändert.

**PRIV-02 – Default deny entlang der Kette.**
*Bezug zum Ist-Stand:* Fail-closed-Muster sind im Code bereits etabliert
(`validate_artifact` vor Speicherung, `parse` in
`runtime/reservations/records.rs` prüft Ersteller, Schema und Identität,
`admission`-Prüfungen in `apps/adapter-host/src/runtime/reason/`).
*Erweiterungsbedarf:* Die Kette **Memory-Retrieval → Kontextbildung →
Transform → Request → Reason → Act → Output** ist als durchgehender
Autorisierungspfad zu führen. Jede Stufe braucht einen Filter vor der
Offenlegung, nicht eine Prüfung nach dem Modellaufruf. Retrieval-Bounds
(`store/retrieval/`, `RetrievalLimits`) sind vorhanden, müssen aber um
Zweck-/Empfängerbezug ergänzt werden.

**PRIV-03 – Konservative Vererbung.**
*Bezug zum Ist-Stand:* `Provenance` erfasst Eltern und angewandte Regeln,
`ContextSelection` wird als Core-Artefakt eingefroren
(`reasoning/context.rs`), Relationen sind typisiert.
*Erweiterungsbedarf:* Abgeleitete Artefakte, Kontextprojektionen, Graphkanten,
Caches, Zusammenfassungen und Logs müssen die **maximale** Schutzanforderung
ihrer Quellen erben. Eine Herabstufung braucht eine explizite,
protokollierte Declassification-Transformation mit Prüfung. Unklarheit führt
zu Ablehnung.

**PRIV-04 – Secret-Broker.**
*Bezug zum Ist-Stand:* Es existiert **kein** Secret-Broker. Der Host begrenzt
die geerbte Prozessumgebung über `apps/adapter-host/src/process_environment.rs`
(`env_clear` plus allowlist-basiertes `inherit_env`) – das ist die
Teilimplementierung von v3 §10.1 SEC-05.
*Erweiterungsbedarf:* Der Broker ist als eigener Core-Dienst zu führen, der
Schlüssel **nicht** in Prompts, Kontexte oder Logs ausgibt, sondern
zweckgebundene, kurzlebige Berechtigungen an konkrete Prozesse ausgibt.
Ein Modell darf niemals einen Schlüsselinhalt sehen.

**PRIV-05 – Policies außerhalb der Modelle.**
*Bezug zum Ist-Stand:* Grenzen existieren als
`AdapterGrant`-Limits (`registration/grant.rs`), `ReasoningLimits`,
`DataLimits`, Ressourcenpools (`resource_policy.rs`) und
Prozessumgebungsregeln (`process_environment.rs`). v3 §10.1 SEC-06 (Isolation
fremden Codes) ist laut Implementierungsplan noch offen.
*Erweiterungsbedarf:* Dateisystem-, Datenbank- und Egress-Regeln sind als
Host-seitige Durchsetzung zu ergänzen, mit **negativen Tests**: Ein
Prompt-Injection-Versuch in einem Toolergebnis darf keine zusätzliche
Rechte erzeugen. Fehlerausgaben, Telemetrie und Suchanfragen sind als
Datenabflüsse zu behandeln.

**PRIV-06 – Löschung, Aufbewahrung, Audit.**
*Bezug zum Ist-Stand:* Unveränderliche, versionierte Artefakte mit Herkunft
(`core/artifact/types.rs`, `store/database/sqlite_database.rs`) und
Zugriffsbelege über `Trust`, `Source`, `Provenance`.
*Erweiterungsbedarf:* Unveränderlichkeit ist kein Argument gegen Löschung:
sensible Nutzlasten müssen **getrennt** von der Beweiskette löschbar sein
(getrennte Payload-Referenz statt Inline-Nutzdaten). Aufbewahrungsfristen,
Audit-Ereignisse und Index-/Cache-Bereinigung sind als Verträge zu
ergänzen. Rechtliche, anbierseitige und vertragliche Anforderungen sind
zusätzlich zu technischen Regeln zu erfüllen und nicht in Code zu ersetzen.

**PRIV-07 – Zulässigkeit vor Qualität im Modellrouting.**
*Bezug zum Ist-Stand:* Das Reasoner-Routing liegt in
`crates/arcs-core/src/reasoning/routing/` mit `reasoner_selection.rs` und
`crates/arcs-core/src/reasoning/selection/`; es wählt heute nach
Verfügbarkeit und Passung, nicht nach Datenklasse.
*Erweiterungsbedarf:* Die Reihenfolge „Zulässigkeit → Qualität → Kosten →
Latenz“ ist im Routing verbindlich zu machen. Lokale Ausführung ist **nicht**
automatisch vertrauenswürdig; ein externer Reasoner darf nur für
ausdrücklich erlaubte Kontexte eingesetzt werden.

**PRIV-08 – Abstraktion vor Offenlegung.**
*Bezug zum Ist-Stand:* Es existiert **keine** Abstraktions- oder
Pseudonymisierungsschicht. Vorhanden sind lokale Kontextprojektion
(`reasoning/context.rs`, `select_payload_fields`) und epistemische Filter.
*Erweiterungsbedarf:* Klassifikation, Pseudonymisierung, Aggregation und
minimale Projektion sind **lokal** vorzuschalten. Pseudonymisierung ist als
nicht-anonym zu kennzeichnen; die Gefahr von Rückschlüssen über mehrere
Abfragen ist mitzubewerten.

**PRIV-09 – Symbolisches Reasoning.**
*Bezug zum Ist-Stand:* Es gibt keine Abstraktionsschnittstelle zu externen
Reasonern. Vorhanden sind Capability-Verträge und isolierte Adapterprozesse
(`crates/arcs-adapter-protocol/src/`), `reasoning.rs`/`reasoning_v2.rs` und
isolierte Ausführung als Ziel von v3 §10.1 SEC-06.
*Erweiterungsbedarf:* Der Austausch von Variablen, Schnittstellen und Regeln
statt Werten ist als eigener, klassifizierter Nachrichtenvertrag zu
definieren. Generierter Code bleibt vor Ausführung geprüft und isoliert; das
Ergebnis behält die Herkunft der Eingabedaten.

**PRIV-10 – Lokale Rekonstruktion und Verification.**
*Bezug zum Ist-Stand:* Der Verification-Pfad liegt in
`crates/arcs-core/src/decision/verification/` und den Gates der Goal-Engine
(`apps/adapter-host/src/task_runtime/goal/gates.rs`).
*Erweiterungsbedarf:* Die aus einer abstrakten Lösung gewonnene konkrete
Anweisung muss **lokal** gegen das reale Problem geprüft werden. Fehlt
entscheidende Information in der Abstraktion, ist auf eine zulässige lokale
Verarbeitung umzuschalten; eine automatische externe Weitergabe ist
ausgeschlossen.

**PRIV-11 – Vertrauliche Berechnung (Forschung).**
*Bezug zum Ist-Stand:* Nicht vorhanden; kein Bestandteil von P0/P1.
*Erweiterungsbedarf:* Bewertung statt Implementierung. Zu messen sind
Erfolgsqualität, offengelegte Information, Reidentifikationsrisiko, Kosten,
Latenz und Fehlfreigaben gegenüber lokalen und nicht-abstrahierten
zulässigen Baselines. Es darf nicht behauptet werden, Cloud-LLM-Reasoning sei
vollständig privat.

### 19.2. Offene Punkte dieser Erweiterung

- Die Frage „welche minimale Information braucht ein Reasoner für eine
  hinreichend zuverlässige Lösung?“ ist als **Forschungsfrage** zu behandeln
  und mit messbaren Baselines zu beantworten (PRIV-08/09/10).
- Ein belastbares Klassenmodell (S0–S4) muss mit dem Policy-Modell aus
  CTX-08 zusammenpassen. Beide sind gemeinsam zu entwerfen, damit
  Artefaktattribut und Zugriffsentscheid nicht auseinanderlaufen.

## 20. Formale Methoden als feste Fähigkeit: Lean

Ergänzt v3 §3.2, §9 und §10. Lean soll sowohl während der Entwicklung als
auch zur Laufzeit genutzt werden können. Der Core definiert die formalen
Prüfverträge und die Verbindlichkeit verlangter Nachweise. Der Reason Manager
nutzt Lean zur logischen Planung und Problemlösung; eine isolierte Lean-Engine
wird über die bestehende Capability-/Adapter-Infrastruktur aufgerufen. Lean
wird **weder ein siebter Manager noch eine zusätzliche oberste
Kontrollinstanz**.

> **Umsetzungsstand 28. November 2026:** Der Core führt formale Aussagen,
> Beweisobligationen, Beweise, Gegenbeispiele und Verifikationsberichte als
> versionierte Artefakte und entscheidet, ob ein Nachweis eine Handlung
> trägt. Lean läuft als externer Adapter in der bestehenden
> Capability-Infrastruktur, erbt weder Autorität noch Store-Zugriff und
> läuft in einem eigenen temporären Arbeitsverzeichnis mit harter
> Laufzeitgrenze. Ein vorgeschriebener Nachweis entscheidet vor dem übrigen
> Handlungsvertrag; fehlt er, wartet das Ziel in einem begrenzten
> Wartezustand, statt zu handeln. `FORM-01`, `FORM-02` und `FORM-05` sind
> umgesetzt, `FORM-03` in der Grundlage. Offen bleiben `FORM-04`, `FORM-06`
> und `FORM-07`. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_FORMALE_NACHWEISE.md).

| ID | Prio | Anforderung |
| --- | --- | --- |
| FORM-01 | P0 | Formale Aussagen und Belege als versionierte Artefakte mit überprüfbarer Herkunft. Typen: `formal_definition`, `formal_assumption`, `formal_model`, `proof_obligation`, `proof`, `theorem`, `counterexample`, `formal_verification_report`. |
| FORM-02 | P0 | Nachweis bindet konkrete Aussage, Annahmen, Modell-/Planversion, Toolchain, erlaubte Axiome und Prüfresultat. Unbewiesene Aussagen, `sorry`, nicht erlaubte Axiome, Timeouts und nicht prüfbare Ergebnisse gelten nicht als verifiziert. Modellgenerierter Beweistext ist erst nach unabhängiger Prüfung Evidenz. |
| FORM-03 | P0 | Sollzustände, Vor-/Nachbedingungen, Invarianten, Abhängigkeiten, widersprüchliche Anforderungen, Ressourcenlimits und logische Planbedingungen formal ausdrücken. Fehlende Beweisobligationen oder konkrete Gegenbeispiele dienen als strukturierte Reasoning-Rückmeldung. |
| FORM-04 | P0 | Sicherheitsmodell des Core formalisieren: Authority/Grant Preservation, Referenzintegrität, Artifact-Lebenszyklus, erlaubte Informationsflüsse, Klassifikationsvererbung, kontrollierte Declassification, Kontextintegrität und Isolation, sichere Zustandsübergänge, Ressourcengrenzen, Bedingungen für Konflikt-/Deadlock-Freiheit. |
| FORM-05 | P1 | Laufzeitnutzung durch vorab verifizierte Prüffunktionen und bei Bedarf dynamische Beweisprüfung über Adapter; begrenzte Laufzeit, Speicher und Capability-Rechte. Bei vorgeschriebenem Nachweis verhindert fehlgeschlagene oder nicht verfügbare Prüfung die Aktion. |
| FORM-06 | P1 | Lean mit SMT-/Constraint-Solvern, Optimierern, Simulationen und gewöhnlichen Tests kombinieren. Solver-Ausgaben und empirische Ergebnisse behalten ihren epistemischen Status. |
| FORM-07 | P2 | Werkzeuge zur Übertragung unterstützter Rust-Teilmengen in formale Modelle evaluieren; keine ungeprüfte Gleichsetzung von extrahiertem Modell und Produktionssystem. |

**FORM-01 – Formale Artefakte.**
*Bezug zum Ist-Stand:* Das Artefaktmodell ist offen für neue Typen: Typ und
Schema werden über `artifact_type` und `schema_id` geführt, die Registry in
`crates/arcs-core/src/core/schema/registry.rs` bindet eingebettete JSON-Schemas
aus `crates/arcs-core/schemas/`. Es existieren **keine** formalen Typen.
*Erweiterungsbedarf:* Acht neue Artefakttypen mit je eigenem Schema sind
ergänzbar, ohne das Versionierungs- und Provenienzmodell zu verändern. Die
Schemas sind in die Registry einzubetten, damit die Prüfbarkeit nicht von
einem veränderbaren Arbeitsverzeichnis abhängt.

**FORM-02 – Verbindlichkeit des Nachweises.**
*Bezug zum Ist-Stand:* Die Verification-Infrastruktur
(`decision/verification/`, Schema `arcs.verification_report.v1`) prüft bereits
Ergebnisse gegen konkrete Versionen; `goal/gates.rs` erzwingt Freigabe vor
Wirkung.
*Erweiterungsbedarf:* Der Nachweis muss dieselbe Strenge erben: ein
`formal_verification_report` ist nur zusammen mit gebundener Aussage,
Annahmen, Toolchain-Version, erlaubten Axiomen und Prüfergebnis gültig.
`sorry` ist im Schema abzuweisen (nicht nur in der Doku zu verbieten).

**FORM-03 – Formale Planbedingungen.**
*Bezug zum Ist-Stand:* Plan-, Abhängigkeits- und Gate-Strukturen liegen in
`crates/arcs-core/schemas/v1|v2|v3|v4/plan.schema.json`,
`apps/adapter-host/src/task_runtime/dependencies.rs` und den Gates.
*Erweiterungsbedarf:* Beweisobligationen und Gegenbeispiele müssen als
**strukturierte Rückmeldung** in den Reasoning- und Planungsprozess
zurückfließen können. Ein fehlender Beweis ist ausdrücklich **kein**
Gegenbeweis.

**FORM-04 – Formales Sicherheitsmodell.**
*Bezug zum Ist-Stand:* Die relevanten Core-Eigenschaften sind in
`core/artifact/types.rs` (Unveränderlichkeit, Provenienz),
`adapters/registration/grant.rs` (Authority), `runtime/reservations.rs`
(exklusive Ressourcen, Revisionen), `reasoning/context.rs` (Sichten) und
`store/` (Referenzintegrität) implementiert.
*Erweiterungsbedarf:* Diese Eigenschaften sind in einem formalen Modell zu
spiegeln und – soweit automatisiert prüfbar – durch vorab verifizierte
Prüffunktionen abzusichern. Noninterference darf **nur** unter explizitem
Beobachter- und Bedrohungsmodell behauptet werden; ohne dieses Modell ist
die Aussage zu unterlassen.

**FORM-05 – Laufzeitprüfung.**
*Bezug zum Ist-Stand:* Adapter sind Prozesse mit Capability- und
Ressourcengrenzen (`AdapterGrant`, `resource_policy.rs`,
`ReasoningLimits`); `v3 §10.1 SEC-06` (Isolation fremden Codes) ist offen.
*Erweiterungsbedarf:* Die Lean-Engine ist ein **externer Adapter** in der
bestehenden Rolle Transform/Reason mit begrenzten Rechten. Sie erhält keinen
Zugriff auf den Kernspeicher, nur auf den übergebenen Kern und ein
Ergebnisartefakt. Fehlt ein vorgeschriebener Nachweis, wird die Aktion
blockiert oder in einen expliziten Wartezustand überführt – nicht
„bestanden angenommen“.

**FORM-06 – Kombination mit anderen Verfahren.**
*Bezug zum Ist-Stand:* Es existieren Solver-/Reasoner-Verträge als austauschbare
Capabilities (`v3 §9.1`, `crates/arcs-adapter-protocol/src/manifest.rs`).
*Erweiterungsbedarf:* Die epistemische Statusfrage ist zu beachten: ein
formaler Beweis über ein Modell garantiert weder die Wahrheit seiner
Annahmen noch die Korrektheit des Rust-Programms noch reale Sicherheit.
Modell-Implementierungs-Abgleich, Refinement, Integrationstests und
adversariale Tests bleiben notwendig.

**FORM-07 – Rust-zu-Modell-Übertragung (Forschung).**
*Bezug zum Ist-Stand:* Nicht vorhanden.
*Erweiterungsbedarf:* Evaluation der Werkzeugkette, keine ungeprüfte
Gleichsetzung von extrahiertem Modell und Produktionssystem.

## 21. Dauerhafter Betrieb, verteilte Ausführung und KI-Mitarbeiter

Ergänzt v3 §7–§10 und §14. Paperclip (Organisation, Heartbeats, Budgetierung
und Task Ownership), Herdr (Sessions, Remote Execution und Agentenstatus) sowie
weitere Harnesses sind **Referenzen zur Evaluation**, keine verbindlichen
Abhängigkeiten und keine vollständig geprüften Codebasen.

| ID | Prio | Anforderung |
| --- | --- | --- |
| OPS-01 | P0 | Dauerhaft laufender Host mit Ereignissen, geplanten Aktivierungen, Heartbeats, Status, Recovery, begrenztem Retry und kontrolliertem Shutdown. Keine permanente LLM-Inferenz während Wartezuständen. |
| OPS-02 | P1 | Verteilte Execution Nodes mit authentifizierter Identität, Fähigkeiten, OS, Kapazität, Trust, Gesundheitszustand und kurzlebigen Grants. Logische Sternarchitektur bleibt unverändert. |
| OPS-03 | P0 | Persistente Task Ownership, Leases/Fencing und Operation IDs verhindern konkurrierende exklusive Änderungen. Wiederaufnahme berücksichtigt bereits erfolgte Wirkungen. Kein pauschales Exactly-once-Versprechen. |
| OPS-04 | P1 | Externe Agenten und Coding-Harnesses (z. B. Codex, Claude Code, OpenCode) sind begrenzte Capabilities mit strukturiertem Input, Status, Evidenz, Resultat, Abbruch und Ressourcenbudget. |
| OPS-05 | P1 | Autorisiertes Operations-Interface für laufende Aufgaben, Abhängigkeiten, Budgets, Kosten, Knoten, Fehler, Journal, Freigaben und menschliche Übernahme. Freigaben gelten nur für die geprüfte konkrete Operation. |
| OPS-06 | P1 | Ein Rollen-/Mitarbeiterprofil ist eine persistente logische Identität mit Zuständigkeiten, Zielen, Policies und nachvollziehbarer Arbeitshistorie. Es benötigt keinen eigenen vollständigen Core. |

**OPS-01 – Dauerbetrieb.**
*Bezug zum Ist-Stand:* Ereignisgesteuerte Aktivierung, Fristen, Heartbeat
und Wiederkehrung existieren in `task_runtime/journal/{scheduling,deadlines,recurrence}.rs`,
`task_runtime/scheduler/recurrence.rs`; Recovery in `task_runtime/recovery.rs`
und `journal/`. Wartezustände liegen im Journal, nicht in Modellaufrufen – die
Forderung „keine permanente LLM-Inferenz im Wartezustand“ ist strukturell
erfüllt und als Vertrag festzuhalten.
*Erweiterungsbedarf:* Der **Host-Lebenszyklus** (Start, Health, begrenztes
Retry, kontrollierter Shutdown) ist als eigener Dienst zu führen. Bestehende
Scheduling-Logik bleibt im Task-Runtime, nicht im Host.

**OPS-02 – Execution Nodes.**
*Bezug zum Ist-Stand:* Adapter laufen als getrennte Prozesse und besitzen
authentifizierte Identitäten und Capabilities (`registration/registry.rs`,
`registration/grant.rs`); `TrustLevel` und `ProducerClass` existieren.
*Erweiterungsbedarf:* Ein Node ist als **Kapazitätsträger** zu modellieren,
nicht als eigener Core. Der logische Stern bleibt: Core entscheidet, Node
ausführt. Kurzlebige Grants sind mit den bestehenden Adapter-Grants
abzugleichen, nicht parallel dazu zu erfinden.

**OPS-03 – Ownership, Leases, Fencing.**
*Bezug zum Ist-Stand:* Es existieren **exklusive Ressourcenreservationen mit
Revisionen und Fences** (`crates/arcs-core/src/runtime/reservations.rs`,
`action/execution/service.rs` mit `ActFence` und `execute_fenced`) sowie
deterministische `invocation_id` (`runtime/invocation/service.rs`).
*Erweiterungsbedarf:* Die Reservierung gilt heute einer **Ressource**, nicht
einer **Task-Ownership**. Für Task-Ownership sind Leases mit Ablauf,
Übernahme durch andere Knoten und Reconciliation zu ergänzen. Verspätete
Resultate sind über die Ausführungsidentität zurückzuweisen (bereits für
Planrevisionen vorhanden). Unklare Wirkungen sind explizit zu klären
(Reconciliation), nicht zu wiederholen.

> **Umsetzungsstand 28. November 2026:** Task-Ownership und das Buch der
> externen Wirkungen sind umgesetzt und nachgewiesen. Eine Aufgabe wird einer
> logischen Runtime über eine Lease mit Ablauf und Fencing-Token zugeordnet;
> ein fremder Eigentümer führt sie in einen Wartezustand statt in eine
> konkurrierende Bearbeitung. Jede externe Wirkung wird als Artefakt mit
> Operation, Aufgabe, Invocation, Zustand und Versuchszahl gebucht. Der Core
> unterscheidet „nicht eingetreten“, „belegt eingetreten“ und „nicht
> zuordenbar“; nur der letzte Fall verlangt die ausdrückliche Klärung des
> Betreibers, die über ein Operator-Kommando erfolgt und die Aufgabe mit
> demselben Auftrag fortsetzt. `OPS-03` ist damit in der Grundlage umgesetzt,
> `OPS-05` für Aufgaben, Wirkungen und Freigaben, `TEST-10` für die Zuordnung,
> die ausbleibende doppelte Wirkung und die ausdrückliche Klärung nachgewiesen.
> Offen bleiben `OPS-01`, `OPS-02`, `OPS-04`, `OPS-06` und die Knoten-, Kosten-
> und Übernahmeteile aus `OPS-05`. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_AUFGABEN_LEASES.md).

**OPS-04 – Externe Harnesses.**
*Bezug zum Ist-Stand:* Adapter- und Capability-Verträge
(`crates/arcs-adapter-protocol/src/`) bilden die natürliche Schnittstelle;
`runtime/sessions.rs` und `runtime/execution.rs` führen entfernte Sitzungen
und Worker.
*Erweiterungsbedarf:* Ein Harness ist ein Adapter mit **begrenztem
Ressourcenbudget**, strukturierter Statusmeldung und Evidenzformat. Remote
Sessions und Computer Use laufen in isolierten Ausführungsumgebungen; native
UI-/Desktop-Zustände brauchen eigene Ressourcenreservierungen (Anschluss an
`runtime/reservations.rs`).

**OPS-05 – Operations-Interface.**
*Bezug zum Ist-Stand:* Es existieren bereits lesbare Journal-Abfragen
(`journal/queries.rs`, `journal/queue.rs`), ein Monitoring-Modul
(`apps/adapter-host/src/monitor/`), Operator-Kommandos
(`apps/adapter-host/src/operator_command.rs`) und die Control-Panel-App.
*Erweiterungsbedarf:* Das Interface muss Budgets, Kosten, Knoten, Fehler,
Journal, Freigaben und menschliche Übernahme **autorisiert** bündeln. Eine
Freigabe gilt ausschließlich für die geprüfte konkrete Operation und darf
**keinen** globalen Administratorzugriff erzeugen.

**OPS-06 – Rollen als logische Identität.**
*Bezug zum Ist-Stand:* Rollen existieren als Adapterrollen im Manifest
(sieben Rollen in `crates/arcs-adapter-protocol/src/manifest.rs`) und als
`Actor`/`ActorType` bei Artefakten. Ein Rollenprofil mit Zielen, Policies und
Arbeitshistorie existiert **nicht**.
*Erweiterungsbedarf:* Ein Mitarbeiterprofil ist ein persistentes Profilartefakt
über den bestehenden Rollen- und Actor-Verträgen. Es darf **keine** eigene
Authority-Quelle sein: Rechte kommen weiterhin aus Grants.

## 22. Unternehmensfähigkeit und allgemeines Business-/Systems-Reasoning

ARCS soll nicht nur isolierte Programmieraufträge erledigen, sondern ganze
betriebliche Prozesse unterstützen: Kundenanfragen, Akquise, Anforderungen,
Projektplanung, Umsetzung, Tests, Betrieb, Support und Berichterstattung.
Dieselben Mechanismen müssen für andere Anwendungsdomänen nutzbar bleiben.

| ID | Prio | Anforderung |
| --- | --- | --- |
| BIZ-01 | P1 | Unternehmensziele, Aufgaben, Rollen, Budgets, Regeln, Stakeholder, Risiken, Fristen und Entscheidungsverantwortung als Artefakte und Policies darstellen. Operative Koordination bleibt heterarchisch. |
| BIZ-02 | P1 | Business-/Requirements-Reasoning rekonstruiert Geschäftsprozesse aus unvollständigen Beschreibungen, trennt bestätigte Anforderungen, Annahmen, abgeleitete Hypothesen und offene Fragen. |
| BIZ-03 | P1 | Product-, UX- und Systems-Reasoning erzeugt Alternativen, prüft Abhängigkeiten, Akzeptanzkriterien, Wirtschaftlichkeit, Ressourcen, Wartbarkeit und Auswirkungen auf andere Prozesse. |
| BIZ-04 | P1 | Eingehende Ereignisse (z. B. Bug Report) können automatisch zu autorisierten Aufgaben werden. Untersuchung, Lösungsvorschlag, Umsetzung, unabhängige Tests, Freigabe und Deployment werden als nachweisbare Schritte behandelt. |
| BIZ-05 | P0 | Autonomiestufen pro Aktion: (a) risikoarme Routinearbeit, (b) bedingt autonom innerhalb festgelegter Budget-, Vertrags-, Preis- und Risikogrenzen, (c) menschliche Entscheidung bei nicht delegierbaren, strategischen oder wesentlichen Risiken. ARCS ist keine Rechtsperson. |
| BIZ-06 | P1 | Formale Regeln, Entscheidungsmodelle, Simulationen und Evidenz können betriebliche Entscheidungen absichern; unsichere Marktannahmen und Prognosen sind nicht allein aufgrund einer formalen Ableitung wahr. |

**BIZ-01 – Organisation als Artefakte.**
*Bezug zum Ist-Stand:* Ziele, Aufgaben und Pläne sind bereits Artefakte
(`goal-round.schema.json`, `plan.schema.json`), Budgets und Ressourcen sind
Pools (`resource_policy.rs`), Rollen sind Manifestrollen. Es gibt **keine**
Organisationsebene mit Stakeholdern, Risiken und Entscheidungsverantwortung.
*Erweiterungsbedarf:* Diese Ebene ist **generisch** zu modellieren, damit sie
nicht zur fest verdrahteten Unternehmenslogik im Core wird (Prinzip 13).
Dieselben Artefakttypen müssen für andere Domänen wiederverwendbar bleiben.

**BIZ-02 – Requirements-Reasoning.**
*Bezug zum Ist-Stand:* Vorhanden sind epistemische Bewertungen
(`memory/records.rs`, `reasoning/context/epistemic.rs`) und die Trennung von
Annahmen und Belegen im Artefaktmodell (`trust`, `provenance`).
*Erweiterungsbedarf:* Die Trennung „bestätigte Anforderung / Annahme /
Hypothese / offene Frage“ ist als eigener Artefaktzustand zu führen und in
Berichten (CTX-09) sichtbar zu machen. Rückfragen sind zu erzeugen, statt
Kundenwünsche wörtlich in Code umzusetzen.

**BIZ-03 – Product-, UX- und Systems-Reasoning.**
*Bezug zum Ist-Stand:* Planvergleich und Abhängigkeitsprüfung existieren in
`task_runtime/planning*`, `dependencies.rs` und `goal/gates.rs`; es fehlen
Wirtschaftlichkeits- und Wartbarkeitskriterien.
*Erweiterungsbedarf:* Diese Kriterien sind als **Bewertungsartefakte** zu
modellieren, nicht als Sonderlogik im Reason Manager. Rückkopplung aus
Nutzerfeedback, Tests und Betriebsevidenz gehört in den Lebenszyklus.

**BIZ-04 – Ereignisgetriebene betriebliche Aufgaben.**
*Bezug zum Ist-Stand:* Der Ereigniseingang ist vorhanden
(`journal/scheduling.rs`, `journal/recurrence.rs`, `task_runtime/queue.rs`),
der Gate-Pfad aus Investigation → Verification → Approval → Execution ist in
`goal/gates.rs` etabliert.
*Erweiterungsbedarf:* Ereignisse müssen **autorisiert** in Aufgaben
überführt werden; ein bestandener Modellvorschlag berechtigt nicht zur
Produktionsänderung (das entspricht der bestehenden Freigabe-Logik und ist als
explizite Regel zu verankern).

**BIZ-05 – Autonomiestufen.**
*Bezug zum Ist-Stand:* Die Gate-Struktur liefert heute binär „Freigabe
erforderlich oder nicht". Budget- und Risikogrenzen existieren als
`resource_policy.rs` und `AdapterGrant`-Limits.
*Erweiterungsbedarf:* Autonomiestufen müssen als **Policy** ausgedrückt
werden, die einer Aktion folgt und im Freigabepfad durchgesetzt wird.
Vertragliche, preisliche, budgetbezogene und risikorelevante Grenzen sind
konfigurierbar zu halten. ARCS ersetzt keine gesetzlich erforderliche
menschliche Verantwortung; das ist im Policy- und Freigabetext festzuhalten.

**BIZ-06 – Entscheidungsabsicherung.**
*Bezug zum Ist-Stand:* Simulations- und Solver-Verträge existieren als
austauschbare Capabilities (v3 §9.1).
*Erweiterungsbedarf:* Der epistemische Status formaler, simulativer und
empirischer Belege ist zu trennen. Erfolg wird an tatsächlichen Ergebnissen,
Qualitätsmaßstäben, Kosten und notwendigen menschlichen Eingriffen gemessen –
nicht an Vollständigkeit formaler Ableitungen.

## 23. Ergänzende Abnahmekriterien und Umsetzung

Neue IDs sind im Geltungsbereich dieser Erweiterung eindeutig. Bestehende
v2-/v3-Aufgaben werden nicht umnummeriert und nicht als erledigt markiert.
`P0` bezeichnet Architektur- und Sicherheitsgrundlagen, `P1` darauf aufbauende
Funktionsnachweise, `P2` optionale Forschung. Die konkrete
Implementierungsreihenfolge ist nach dem Abgleich mit dem aktuellen Code
festzulegen; die nachstehenden Slices sind Empfehlungen, keine Zusicherung.

### 23.1. Ergänzende Abnahmekriterien

| Test | Abnahmekriterium | Betroffene IDs |
| --- | --- | --- |
| TEST-06 – Assistenz während Unternehmensarbeit | Ein laufender Hintergrundtask mit externem Prozess bleibt aktiv, während dieselbe ARCS-Instanz eine persönliche Anfrage beantwortet und einen autorisierten Statusbericht liefert. Anschließend korrekte Fortsetzung ohne zweiten Core oder Kontextvermischung. | CTX-01, CTX-03, CTX-05, CTX-09, CTX-10 |
| TEST-07 – Kontrolliertes gemeinsames Memory | Autorisierte bereichsübergreifende Abfrage gelingt; nicht autorisierte private oder fremde Unternehmensdaten bleiben auch über Retrieval, Graphbeziehungen, Cache, Zusammenfassung und Logs unzugänglich. | CTX-08, CTX-10, PRIV-01, PRIV-02, PRIV-03 |
| TEST-08 – Vertrauliches Reasoning | Markierte Testdaten dürfen bei nicht erlaubter Cloud-Verarbeitung weder direkt noch abgeleitet über Modell, Shell, Browser, Logs oder Netzwerk abfließen. Zulässige abstrahierte Aufgabe wird extern bearbeitet und lokal rekonstruiert. | PRIV-04, PRIV-05, PRIV-07, PRIV-08, PRIV-09, PRIV-10 |
| TEST-09 – Formale Prüfung | Gültiger Nachweis wird angenommen; falsche, unvollständige, veraltete oder auf unzulässigen Annahmen/Axiomen basierende Nachweise werden nicht als gültig akzeptiert. Ein vorgeschriebener fehlender Nachweis blockiert die Wirkung. | FORM-01, FORM-02, FORM-05, BIZ-05 |
| TEST-10 – Verteilte Recovery | Node-/Host-Ausfall und verspätete Resultate führen weder zur falschen Task-Zuordnung noch zur unkontrollierten Wiederholung einer externen Wirkung. Unklare Wirkung wird explizit geklärt. | CTX-05, OPS-01, OPS-03 |
| TEST-11 – Betrieblicher Prozess | Ein unvollständiger Kundenauftrag wird in belegte Anforderungen, Hypothesen, Rückfragen, Plan, Umsetzung und überprüfte Ergebnisse überführt; eine nicht autorisierte Produktionsänderung oder Budgetüberschreitung wird blockiert. | BIZ-01, BIZ-02, BIZ-04, BIZ-05, CTX-09 |
| TEST-12 – Ressourcen und Fairness | Mit zunehmenden aktiven und wartenden Kontexten werden RAM/VRAM, Tokenkosten, Antwortlatenz, Durchsatz und Wiederaufnahmekosten gemessen. Vergleich mit getrennten Vollinstanzen; definierte Ressourcenlimits, faire Ausführung und interaktive Reaktionsfähigkeit werden nachgewiesen. | CTX-04, CTX-07, OPS-01 |

> **Umsetzungsstand BIZ-05 (28. November 2026):** Die drei Autonomiestufen
> sind als Betreiberregel im Core umgesetzt. `Routine`, `Bounded` und
> `HumanDecision` werden aus Fähigkeitsregel, Umgebungsgrenze, Risiko-,
> Preis-, Währungs- und Budgetgrenzen abgeleitet; ein Vorschlag kann weder
> sein Risiko herabstufen noch den Preis unter die Betreibererwartung drücken.
> Eine autonome Freigabe entsteht als System-Approval über die bestehende
> Approval-Kette und setzt weiterhin einen Prüfbericht voraus; die
> bestehende Action- und Execution-Kette bleibt unverändert. Das
> Kostenbudget ist ein Core-Artefakt mit optimistischem Vergleich, sodass
> parallele Aufgaben die Grenze nicht überschreiten. Nicht delegierbare
> Regeln bilden gesetzliche, vertragliche und betreiberpolitische Grenzen ab;
> ARCS entscheidet in diesen Fällen nicht. Nicht belegt ist der vollständige
> betriebliche Prozess aus `TEST-11`; die Bausteine Budgetüberschreitung und
> unautorisierte Wirkung sind nachgewiesen. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_AUTONOMIE.md).
>
> **Umsetzungsstand TEST-08 (28. November 2026):** Der erste Teil des Tests
> ist nachgewiesen: Markierte Werte erreichen weder das Artefaktnetz noch
> einen externen Adapter, und ein Auftrag ohne passende Freigabe wird nicht
> gesendet. Der zweite Teil ist mit dem achten Slice nachgewiesen: Eine
> vertrauliche Anfrage wird lokal abstrahiert, der externe Reasoner sieht
> nur Struktur und Platzhalter, und die konkrete Lösung entsteht lokal und
> erbt die Schutzstufe der Quelle. Noch offen ist die Prüfung über den
> Adapterprozess und das Netzwerk (Shell, Browser, Telemetrie).

**Umsetzungshinweise zur Testbarkeit.**

- Die vorhandene Testinfrastruktur (`apps/adapter-host/tests/`, gemeinsame
  Helfer in `apps/adapter-host/tests/support/{goals,plans,memory,parallel}.rs`)
  ist zu erweitern, nicht zu duplizieren. TEST-06/07/08 benötigen echte
  Parallelität im Host und deshalb die vorhandenen Parallel-Testmuster aus
  `tests/parallel_runtime.rs` und `tests/parallel_requests.rs`.
- TEST-08 verlangt Egress-Kontrolle. Bis PRIV-05 umgesetzt ist, ist der Test
  nur mit lokalen Reasonern und Dummy-Endpunkten belastbar; Netzwerkprüfung
  erfolgt über den Adapterprozess und die Prozessumgebung.
- TEST-12 verlangt eine Vergleichsbaseline gegen getrennte Vollinstanzen. Ohne
  die Messgrößen aus CTX-07 ist der Vergleich nicht aussagekräftig; die
  Messung ist deshalb Teil von CTX-07 und nicht von TEST-12 allein.

### 23.2. Empfohlene vertikale Slices

Die folgenden fünf Slices sind die empfohlene Reihenfolge. Jeder Slice ist
so zu schneiden, dass er den bestehenden Vertrag **erweitert** und einen
benannten Test liefert.

**Slice 1 – Vertragsabgleich (Voraussetzung für alle weiteren Slices).**
Ziel: Keine parallele Ersatzarchitektur. Bestehende Goal-Runtime,
Task-Queues, Journale, Ressourcenreservierungen, Memory-Scopes,
Admission-Regeln und Adapterverträge werden gegen CTX-01, CTX-05, CTX-08,
PRIV-01/02 und OPS-03 abgeglichen und in einer Zuordnungstabelle im
Implementierungsplan festgehalten. Ergebnis ist eine Liste konkreter
Erweiterungsstellen, kein Code.
Betroffene Stellen: `apps/adapter-host/src/task_runtime/**`,
`crates/arcs-core/src/runtime/{invocation,reservations}/**`,
`crates/arcs-core/src/reasoning/context.rs`,
`crates/arcs-core/src/core/{artifact,schema}/**`.

**Slice 2 – Mehrere persistente Arbeitskontexte (CTX-01, CTX-03, CTX-05).**
Ziel: Mehrere leichte Arbeitskontexte mit gemeinsamer Infrastruktur und
autorisierten systemweiten Statusabfragen. Serielle Abhängigkeiten,
parallele Hintergrundarbeit und eine interaktive Anfrage laufen
nebeneinander. Kein zweiter Core, keine Vermischung von Session- oder
KV-Zuständen.
Abnahme: TEST-06.

**Slice 3 – Sicherheitsslice Datenschutz (PRIV-01 bis PRIV-05, PRIV-07).**
Ziel: Durchgängige Datenflussprüfung, Herkunft, Secret-Broker,
Egress-Isolation und zulässigkeitsbasiertes Modellrouting. Reihenfolge im
Slice: erst Klassifikation und Vererbung (PRIV-01/03), dann die
Autorisierungskette (PRIV-02), dann Secret-Broker und Egress (PRIV-04/05),
zuletzt das Routing (PRIV-07).
Abnahme: TEST-07 und TEST-08.

**Slice 4 – Formales Kernmodell mit isoliertem Lean-Adapter (FORM-01, FORM-02,
FORM-04, FORM-05).** Ziel: Kleines formales Core-/Policy-Modell, ein
isolierter Lean-Prüfadapter mit positiven und negativen Nachweisen. Der
Adapter arbeitet ohne Zugriff auf den Kernspeicher; die Modellgrenzen
werden dokumentiert (ein Beweis über ein Modell ist kein Beweis über das
Rust-Programm).
Abnahme: TEST-09.

**Slice 5 – Ereignisbasierte betriebliche Aufgaben, verteilte Nodes und externe
Harnesses (OPS-01, OPS-03, OPS-04, BIZ-01, BIZ-04, BIZ-05).**
Ziel: Betriebliche Aufgaben unter denselben Verträgen qualifizieren;
Task-Ownership mit Leases und Reconciliation; externe Harnesses als
begrenzte Capabilities. Cloud-Reasoning-Abstraktion und
Business-/Systems-Reasoning werden an realen, freigegebenen Testfällen
bewertet, nicht an Papierfällen.
Abnahme: TEST-10, TEST-11; TEST-12 begleitend zu Slice 2 und 5.

### 23.3. Traceability der Erweiterung auf das bestehende Zielbild

| Erweiterung | Bezug auf v3 | Vorhandene Verträge, die erweitert werden |
| --- | --- | --- |
| CTX-01, CTX-03, CTX-05 | v3 §7 (Parallelität), §8 (Langzeitautonomie), §5 (Heterarchie) | Task-Journal, Planrevisionen, `InvocationSpec`, Provenance, Action-Fences |
| CTX-02 | v3 §3.2 (Zuständigkeiten des Core), §9 (Routing) | Adapter-Registry, `AdapterGrant`, Verifier, Artifact-Store |
| CTX-04, CTX-07 | v3 §7.2 (Scheduler), §14 (nichtfunktionale Anforderungen) | Scheduler, Prioritäten, Ressourcenpools, Reservierungen |
| CTX-08 bis CTX-11 | v3 §6 (Artifact-Netz), §6.5 MEM-08 | `Artifact`, `Provenance`, typisierte Relationen, `MemoryScope`/`MemoryPolicy` |
| PRIV-01 bis PRIV-06 | v3 §10.1 SEC-01 bis SEC-10, §6.5 | `Trust`, `Source`, Grants, `DataLimits`, `process_environment.rs` |
| PRIV-07 bis PRIV-10 | v3 §9 (Routing), §10.1 SEC-06 | Reasoner-Routing, Capability-Verträge, Isolation |
| FORM-01 bis FORM-07 | v3 §3.2, §9, §10 | Schema-Registry, `verification_report`, Plan-/Gate-Strukturen |
| OPS-01 bis OPS-06 | v3 §7–§10, §14 | Scheduling/Recurrence, Recovery, Reservations, Operator-Kommandos |
| BIZ-01 bis BIZ-06 | v3 §5 (Heterarchie), §8 (Ziele), §10.1 | Artefaktmodell, Gates, epistemische Bewertung, Budgetpools |

### 23.4. Ausdrücklich nicht zu bauen

- Keine zweite Kerninstanz, kein siebter Manager, keine zusätzliche
  Kontrollhierarchie über den sechs Managern.
- Kein prozess- oder aufgabenweise duplizierter Core, Store oder Adaptervertrag.
- Kein global veränderlicher „aktueller Kontext“.
- Kein Ersatz für die vorhandene Memory-, Reasoner-, Goal- oder Task-Runtime.
- Keine Behauptung, dass ein Lean-Beweis über ein Modell die Korrektheit des
  Produktionscodes oder reale Sicherheit garantiert.
- Keine Behauptung, dass Pseudonymisierung Anonymität garantiert.
- Kein pauschales Exactly-once-Versprechen für externe Systeme.
