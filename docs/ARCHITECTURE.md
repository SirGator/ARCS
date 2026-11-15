ARCS – Technische Gesamtarchitektur und zukünftige Anforderungen
Architekturspezifikation v3.0 · 15. November 2026
Technischer Entwurf mit Code- und Zeilenreferenzen
1. Das verbindliche Zielbild
ARCS soll ein universeller, heterarchisch organisierter, parallel arbeitender und langfristig autonomer KI-Agent werden.

Das System soll als persönlicher Assistent funktionieren, aber nicht auf typische Assistentenaufgaben beschränkt sein. Es soll unter anderem Software entwickeln, Daten analysieren, Geräte steuern, Drohnenmissionen durchführen und Projekte über Monate bearbeiten können.

Die bisher festgelegten Architekturprinzipien sind:
1. Ein logischer Core: Keine zusätzliche übergeordnete Agenten- oder Kontrollinstanz.
2. Sternarchitektur: Sechs spezialisierte Manager verbinden den Core mit den externen Systemen.
3. Heterarchie: Fähigkeiten können flexibel, gleichrangig und aufgabenabhängig zusammenarbeiten, ohne eine feste Befehlsreihenfolge.
4. Artifact-Gedächtnis: Das gemeinsame Artifact-Netz dient als langfristiger Wissens-, Erfahrungs- und Aufgabenspeicher.
5. Verpflichtende Parallelität: Mehrere Aufgaben, Teilprozesse, Modelle und Werkzeuge müssen langfristig gleichzeitig arbeiten können.
6. Austauschbare Intelligenz: LLMs, Jev, Solver und weitere Reasoner bleiben externe Komponenten.
7. Kontrollierte Autonomie: Freie Problemlösung ist möglich, aber Berechtigungen, Verification und Approval gelten weiterhin.
8. Einfache Bedienung: Der Benutzer beschreibt Ziele. Die technische Konfiguration soll möglichst automatisch erfolgen.
Das sind Zielanforderungen, keine Behauptung, dass sämtliche Funktionen bereits implementiert wären.
Aktuell überprüfte Codebasis
Das öffentlich sichtbare Repository steht weiterhin bei Commit 23d97ef vom 25. Oktober 2026. Die folgenden Datei- und Zeilenangaben beziehen sich ausdrücklich auf diesen Commit.

Eine wichtige Erkenntnis aus dem Code: Das Artifact-Netz hat bereits gewichtete Verbindungen und Aktivierungspropagation. Zusätzlich existieren semantisch typisierte Beziehungen, die ausdrücklich unabhängig von den Aktivierungsgewichten sind.

Das ist eine geeignete technische Ausgangslage für dein Gedächtniskonzept. Es wäre falsch, diese vorhandene Struktur einfach durch ein neues Memory-System zu ersetzen.
2. Was der vorhandene Code bereits abdeckt
Diese Zuordnung ist für die Weiterentwicklung wichtig, damit vorhandene Funktionen nicht doppelt implementiert werden.
Bestehende Datei	Relevante Zeilen	Aktuelle Funktion
crates/arcs-adapter-protocol/src/manifest.rs	14–24, 79–138, 179–194	Sieben Rollen, Capability-Verträge, Adapter-Manifeste
crates/arcs-core/src/adapters/registration/registry.rs	70–96, 260–363	Registrierung, Capability-Katalog, autorisierte Fähigkeiten
crates/arcs-core/src/adapters/registration/grant.rs	15–40	Berechtigungen, Trust und Ressourcenlimits
crates/arcs-core/src/core/artifact/types.rs	139–173	Artifact-Datenmodell mit Herkunft und Metadaten
crates/arcs-core/src/store/database/sqlite_database.rs	81–151	Persistente Artefakte, gewichtete Kanten, semantische Beziehungen
crates/arcs-core/src/store/network/artifact_network.rs	35–161	Netzwerkverknüpfung und Aktivierungspropagation
crates/arcs-core/src/store/relation.rs	35–94	Typisierte Beziehungen zwischen Artefakten
crates/arcs-core/src/reasoning/routing/mod.rs	100–141	Bekannter Artifact-Fast-Path mit Reasoning-Fallback
apps/adapter-host/src/runtime.rs	213–271, 383–456, 512–735	Adapterzugriff, Reasoning, Request, Transform und Action
apps/adapter-host/src/task_runtime/mod.rs	74–169	Derzeit überwiegend serielle Aufgabenverarbeitung
apps/adapter-host/src/task_runtime/goal/engine.rs	76–372	Persistente, begrenzte Zielschleife
apps/adapter-host/src/task_runtime/goal/gates.rs	131–352	Verification, Approval und Ausführung

Die wichtigsten Quellstellen sind direkt nachlesbar:
Artifact Network – Zeilen 35–161Capability Registry – Zeilen 260–363Goal Engine – Zeilen 76–372Drei besonders wichtige technische Erkenntnisse
A. Das Artifact-Gedächtnis besitzt schon zwei unterschiedliche Beziehungssysteme.

Die Tabelle artifact_edges speichert gewichtete Verbindungen. artifact_relations speichert typisierte semantische Beziehungen. Beides existiert bereits in sqlite_database.rs, Zeilen 102–130.

Das bedeutet: Gedächtnisrelevanz und belegbare Herkunft lassen sich unterschiedlich behandeln. Ein hoher Aktivierungswert muss beispielsweise nicht bedeuten, dass eine Information wahr oder für eine Aktion autorisiert ist.

B. Eine erste assoziative Aktivierung existiert bereits.

ArtifactNetwork::propagate_many in Zeilen 112–161 verarbeitet mehrere aktivierte Quellen, summiert gewichtete Beiträge und sortiert die Ergebnisse.

Allerdings werden die Verbindungen derzeit nur einen Schritt weit ausgewertet, und die Aktivierungen werden nicht gespeichert. Eine allgemeine mehrstufige, kontextbewusste Gedächtnissuche ist noch nicht implementiert.

C. Die bestehende Zielschleife ist noch keine allgemeine parallele Plan-Engine.

TaskRuntime::tick in Zeilen 74–169 verarbeitet Aufgaben schrittweise. Die Goal-Engine erwartet bei der Planung genau einen Handlungskandidaten und arbeitet mit einer konfigurierten Aktionsmenge.

Diese Implementierung sollte als funktionierender Ausgangspunkt erhalten bleiben. Für Heterarchie, dynamische Teilaufgaben und Parallelität muss sie aber wesentlich erweitert werden.
3. Die vollständige Zielarchitektur
Die Architektur sollte aus vier logisch unterscheidbaren Bereichen bestehen. Es sind jedoch keine vier übereinanderliegenden Kontrollinstanzen.
ARCS Core
Goals & Plans

Ziele und Arbeitsabläufe


Artifact Network

Gemeinsames Gedächtnis


Task Runtime

Ausführung und Scheduling


Coordination

Heterarchie und Ressourcen


World Model

Zustände und Beobachtungen


Verification

Kontrolle und Freigaben





Sechs gleichrangige Schnittstellen-Manager
Input


Observe


Request


Reason


Act


Output



Gemeinsames Protokoll, Registry, Grants und Transport – aber getrennte Verantwortlichkeiten


Externe Adapter

Unabhängige Prozesse und Treiber · Transform bei Bedarf


Außenwelt
KI-Modelle


Drohnen


Roboter


Internet


Datenbanken


Software





3.1. Logische Sternarchitektur versus physische Ausführung
Die Sternarchitektur beschreibt die Zuständigkeiten und Schnittstellen, nicht zwingend sämtliche physischen Datenwege.

Es soll drei voneinander unterscheidbare Flüsse geben:
Fluss	Bedeutung	Beispiel
Kontrollfluss	Aufgaben, Berechtigungen und Freigaben	Core beauftragt eine erlaubte Drohnenbewegung
Informationsfluss	Artefakte, Beobachtungen, Ergebnisse	Kamera liefert einen Bildnachweis
Datenfluss	Übertragung großer Rohdaten	Bildbytes werden autorisiert direkt an ein Vision-Modell geliefert

Der Core soll immer entscheiden können, ob ein Datenzugriff oder eine Aktion erlaubt ist. Er muss aber nicht jeden Byte eines Videostreams selbst verarbeiten.

Das verhindert, dass aus dem logischen Zentrum ein physischer Leistungsengpass wird.
3.2. Zuständigkeiten des Core
Funktion	Verbindliche Verantwortung
Goal Management	Ziele und Teilziele dauerhaft verwalten
Plan Management	Pläne als überprüfbare Zustände und Abhängigkeiten führen
Artifact Network	Wissen, Erfahrungen, Ergebnisse und Zusammenhänge speichern
World Model	Den aktuellen geschätzten Weltzustand verwalten
Scheduling	Aufgaben bei Zeit- oder Ereignisauslösern aktivieren
Coordination	Zusammenarbeit und Ressourcenkonflikte koordinieren
Verification	Formale, inhaltliche und anwendungsspezifische Prüfungen koordinieren
Approval	Berechtigte Freigaben erfassen und durchsetzen
Runtime	Schritte ausführen, pausieren, fortsetzen und überwachen

Diese Funktionen sollen getrennte Module oder Dienste innerhalb derselben logischen ARCS-Steuerung sein.

Architekturregel CORE-01: Kein Modul erhält allein deshalb mehr Autorität, weil es einen Plan erzeugt oder einen anderen Ablauf koordiniert.

4. Die sechs Manager – genaue Anforderungen
Die Manager sollen keine sechs identischen Implementierungen derselben Infrastruktur sein.

Sie besitzen gemeinsam genutzte technische Grundlagen, aber unterschiedliche operative Verantwortlichkeiten.
Manager	Verantwortlichkeiten	Nicht seine Aufgabe
Input	Ereignisannahme, Identität, Validierung, Deduplizierung, Backpressure	Aufgaben eigenständig lösen
Observe	Sensorstreams, Zeitbezug, Zustandsbeobachtungen, Qualitätsangaben	Sensordaten ungeprüft als Wahrheit festlegen
Request	Zielgerichtete Abfragen, Timeout, Ergebnisbindung, Caching	Selbstständig Ziele erfinden
Reason	Reasoner-Auswahl, Modellaufrufe, Budget, Vergleich, Fallback	Aktionen selbst genehmigen
Act	Geprüfte Aktionen dispatchen, Ausführung überwachen, Replay	Sicherheitsfreigaben umgehen
Output	Ergebnisse übermitteln, Zustellung prüfen, wiederholen	Beliebige externe Aktionen ohne Berechtigung ausführen

4.1. Anforderungen an alle Manager
MGR-01 – Gemeinsame Identitäten. Alle verwenden dieselben AdapterId, CapabilityId und CapabilityRef.

MGR-02 – Gemeinsame Autoritätsgrundlage. Grants werden nicht zwischen Managern kopiert und unabhängig interpretiert.

MGR-03 – Unabhängige Zuständigkeiten. Ein Problem im Reason Manager darf nicht automatisch die Verwaltung der übrigen Adapter verändern.

MGR-04 – Parallel nutzbare Schnittstellen. Jeder Manager muss künftig mehrere unabhängige Operationen gleichzeitig bedienen können.

MGR-05 – Fehlerisolation. Der Ausfall eines Adapters muss eindeutig sichtbar sein, ohne andere Adapterzustände zu verfälschen.

MGR-06 – Multi-Rollen-Adapter. Eine Adapterinstallation kann gleichzeitig Observe, Request und Act anbieten.

MGR-07 – Kein verpflichtendes Transform. Daten werden nur dann umgewandelt, wenn eine fachlich notwendige Transformation vorliegt.

MGR-08 – Einheitliches Monitoring. Status, Fehler, Latenzen, Ressourcenverbrauch und Ausführungsbelege müssen vergleichbar erfasst werden.
4.2. Technische Umsetzung
Der aktuelle Host besitzt bereits Schnittstellen für unterschiedliche Operationen. Beispielsweise:
- Host::reason_with_data: runtime.rs, Zeilen 245 ff.
- Host::request: Zeilen 512 ff.
- Host::transform: Zeilen 614 ff.
- Host::execute_approved: Zeilen 709 ff.
- Host::adapter_catalog: Zeilen 227 ff.
Daher würde ich nicht sechs neue vollständige Kommunikationssysteme entwickeln.

Stattdessen könnte der Host intern eine Struktur dieser Art erhalten:
apps/adapter-host/src/
    managers/
        mod.rs
        input.rs
        observe.rs
        request.rs
        reason.rs
        act.rs
        output.rs
    runtime.rs
    task_runtime/


Vorgeschlagene zukünftige Dateistruktur; diese Manager-Dateien sind keine bereits bestätigten Repository-Dateien.Die Manager können vorhandene Core-Dienste und Adapter-Sessions verwenden. Die tatsächliche Trennung wird durch eindeutige Schnittstellen, Zuständigkeiten und Tests gewährleistet.

5. Heterarchie – das neue Kooperationsprinzip
Das ist eine der grundlegendsten neuen Anforderungen.
5.1. Was Heterarchie in ARCS bedeutet
Eine Heterarchie ist hier eine Struktur, in der mehrere Fähigkeiten ohne feste funktionale Rangordnung kooperieren können.

Der Reason Manager muss beispielsweise nicht jede Operation anstoßen.

Eine Observation kann eine neue Bewertung auslösen. Eine Datenbankabfrage kann dazu führen, dass ein bisheriger Plan ungültig wird. Ein Solver kann einen Teilplan berechnen, während ein LLM einen anderen Teil bearbeitet.

Die Reihenfolge entsteht aus den tatsächlichen Abhängigkeiten der Aufgabe, nicht aus einer universellen Abfolge aller Adapter-Rollen.
Beispiel: Heterarchische Zusammenarbeit
LLM

Entwickelt Suchstrategie


Solver

Berechnet mögliche Wege




Gemeinsames Planartefakt

Beiträge und Abhängigkeiten werden zusammengeführt


Vision

Prüft Beobachtungen


Drohnenadapter

Führt freigegebene Schritte aus




Hier ist keine KI grundsätzlich der Vorgesetzte einer anderen.

Der Core verwaltet aber weiterhin die gemeinsamen Zustände, zulässigen Abhängigkeiten und Berechtigungen.
5.2. Verbindliche Heterarchie-Anforderungen
ID	Anforderung
HET-01	Aufgabenabhängige Zusammenarbeit zwischen unterschiedlichen Capabilities
HET-02	Dynamische Beteiligung und Ablösung von Reasonern
HET-03	Parallele Bearbeitung unabhängiger Teilziele
HET-04	Zusammenführung unterschiedlicher Zwischenergebnisse
HET-05	Nachvollziehbarer Umgang mit widersprüchlichen Vorschlägen
HET-06	Neuplanung bei veränderten Umständen
HET-07	Wiederverwendbare Kooperationsmuster
HET-08	Kein automatischer Autoritätsgewinn durch Delegation
HET-09	Schutz vor zyklischen Abhängigkeiten und Ressourcen-Deadlocks
HET-10	Deterministische Konfliktentscheidung bei konkurrierenden exklusiven Aktionen

5.3. Wie werden heterarchische Aufgaben dargestellt?
Ich würde Pläne als Artefaktgraphen darstellen.

Ein Plan kann mehrere Teilaufgaben enthalten. Eine Teilaufgabe kann wiederum von mehreren Voraussetzungen abhängig sein. Unabhängige Zweige dürfen gleichzeitig laufen.

Beispiel:
Ziel: Drohne soll einen Raum kartieren

Teilziel A

Kamera kalibrieren


Teilziel B

Suchstrategie entwickeln




Teilziel C

Erkundung ausführen, wenn A und B erfüllt sind

Teilziel D

Ergebnisse prüfen und dokumentieren


Die Teilziele A und B können parallel bearbeitet werden. C benötigt ihre Ergebnisse.

Eine solche Struktur kann als DAG – gerichteter azyklischer Graph – die normalen Ausführungsabhängigkeiten repräsentieren. Wiederholungen und Umplanung sollten darüber als explizite Zustandsübergänge beziehungsweise neue Planversionen abgebildet werden.

Wichtig: Heterarchie bedeutet nicht, dass jedes Programm unmittelbar jedes andere aufrufen darf. Kooperation kann über autorisierte Aufträge, Artefakte und Ereignisse erfolgen, während die Sicherheit zentral konsistent bleibt.

6. Das Artifact-Netz als echtes Langzeitgedächtnis
Hier würde ich besonders viel Entwicklungsarbeit investieren, weil dieser Bereich deine gesamte Architektur verbindet.
6.1. Vorhandenes Datenmodell
Artifact enthält bereits unter anderem:
- artifact_id, version_id, version
- schema_id, schema_version
- created_at, created_by
- source, trust
- subject, tags, payload
- provenance
Diese Felder stehen in crates/arcs-core/src/core/artifact/types.rs, Zeilen 139–173.

Es gibt ebenfalls bereits semantische Beziehungen wie caused_by, derived_from, supported_by, verifies, based_on und evaluates.
Vorhandene Relationsarten im Code6.2. Die fünf zusätzlichen Gedächtnisfähigkeiten
Funktion	Zukünftige Anforderung	Beispiel
Encoding	Ereignisse in verständliche, verknüpfte Artefakte überführen	Fehlgeschlagener Drohnentest
Association	Semantische und empirische Beziehungen herstellen	Fehler ähnelt früherem Fehler
Retrieval	Für aktuelle Ziele relevante Artefakte finden	Frühere Lösung wiederfinden
Consolidation	Wiederkehrende Erkenntnisse ableiten	Bewährte Kalibrierstrategie
Revision	Neue Erkenntnisse gegen alte Annahmen abgleichen	Veraltete Sensordaten erkennen

6.3. Gedächtnisabruf als mehrstufiges Verfahren
Eine mögliche Retrieval-Pipeline wäre:
Anfrage verstehen

Aktuelles Ziel und relevanten Kontext bestimmen

Kandidaten suchen

IDs, Zeit, Metadaten und semantische Ähnlichkeit

Graph erweitern

Direkt und indirekt verbundene Erinnerungen finden

Relevanz bewerten

Aufgabenbezug, Aktualität, Herkunft und Nutzen

Widersprüche prüfen

Konfliktierende oder überholte Aussagen markieren

Kontext zusammenstellen

Begrenzte, belegbare Informationen für Reasoning


Ein Ranking könnte mehrere Signale verwenden:
\[
R(a,q)=w_sS+w_gG+w_tT+w_eE
\]

Dabei ist \(S\) semantische Ähnlichkeit, \(G\) Graphrelevanz, \(T\) zeitliche Relevanz und \(E\) empirisch gemessener Nutzen früherer Abrufe.

Die Formel ist ein vorgeschlagenes Forschungsmodell, keine vorhandene ARCS-Implementierung.

Die Herkunfts- und Berechtigungsprüfung würde ich nicht einfach in diesen Score mischen. Sie sollte als harte Zugriffsgrenze davor beziehungsweise daneben stehen.
6.4. Aktive Erinnerung versus überprüfte Wahrheit
Eine wichtige Architekturregel:

Hohe Aktivierung bedeutet hohe Abrufrelevanz, nicht hohe Wahrheit.

Ein stark aktivierter früherer Fehlerbericht kann äußerst relevant sein. Er darf aber nicht dadurch automatisch als gültiger aktueller Weltzustand betrachtet werden.

Daher würde ich zwei Mechanismen getrennt lassen:
Mechanismus	Funktion
Gewichtete Kanten	Assoziative Verbindungen und Abrufrelevanz
Typisierte Relationen	Nachvollziehbare fachliche Beziehungen und Belege

Hier passt der bestehende Code bereits gut zum Ziel.
6.5. Zusätzliche zukünftige Datenanforderungen
MEM-01: Mehrstufige Graphsuche mit begrenzter Tiefe, Kandidatenzahl und Laufzeit.

MEM-02: Effiziente Metadaten- und Volltextindizes sowie optionale semantische Suchindizes.

MEM-03: Erinnerungseinträge müssen auf ihre ursprünglichen Artefaktversionen zurückführbar sein.

MEM-04: Zeitliche Gültigkeit und epistemische Unsicherheit explizit erfassen.

MEM-05: Alte Fakten nicht überschreiben, sondern nachvollziehbar korrigieren beziehungsweise ersetzen.

MEM-06: Erfolgreiche Handlungsmuster als wiederverwendbare, überprüfbare Artefaktstrukturen darstellen.

MEM-07: Beziehungen anhand von Ergebnissen aktualisieren, ohne Fehlschlüsse aus bloßer Korrelation automatisch als Tatsachen zu behandeln.

MEM-08: Retrieval muss den jeweiligen Zugriffsrechten entsprechen.

MEM-09: Speicherung und Suche müssen über Monate auch bei stark wachsender Artefaktanzahl effizient bleiben.

MEM-10: Gedächtnisabrufe müssen messbar gegen einfachere Baselines getestet werden.
6.6. Wo würde ich den Code erweitern?
Aktuelle Datei	Änderung
store/network/artifact_network.rs	Mehrstufige, begrenzte Graphauswertung
store/relation.rs	Weitere validierte semantische Beziehungen
store/database/sqlite_database.rs	Indizes, effizientes Retrieval, transaktionale Updates
reasoning/routing/mod.rs	Gedächtnis- und Fast-Path-Ergebnisse als Kontextquellen kombinieren
runtime/agent_cycle/mod.rs	Gedächtnisabruf in die kontrollierten Agentenzyklen integrieren

Neue Rust-Module für Retrieval oder Konsolidierung wären möglich, sollten aber denselben Artifact Store verwenden.

Ein zweites unabhängiges Memory-System mit eigenen widersprüchlichen Faktenbeständen würde ich vermeiden.

7. Parallele Runtime – langfristig verpflichtend
Parallelität muss in ARCS zu einer echten Systemeigenschaft werden.

Das umfasst nicht nur mehrere gleichzeitig aktive Aufgaben, sondern auch parallele Arbeitsschritte und parallele externe Berechnungen.
7.1. Vier Ebenen der Parallelität
Ebene	Beispiel	Technische Voraussetzung
Aufgaben	Softwareprojekt und Recherche gleichzeitig	Unabhängige Task-Zustände
Teilaufgaben	Tests, Analyse und Dokumentation parallel	Abhängigkeitsgraph
Reasoning	Zwei Modelle entwickeln Alternativvorschläge	Parallele Invocations
Adapter	Kamera, Datenbank und Modell arbeiten gleichzeitig	Nebenläufige Kommunikation

Eine fünfte Eigenschaft ist besonders wichtig: Tausende langfristige Aufgaben können gleichzeitig existieren, während viele davon schlafen und nur wenige tatsächlich rechnen.
7.2. Zukünftige Scheduler-Anforderungen
ID	Anforderung
PAR-01	Persistente, unabhängig ausführbare Tasks
PAR-02	Begrenzte Worker-Pools statt unbegrenzter Threads
PAR-03	Parallele Teilaufgaben mit Abhängigkeiten
PAR-04	Ressourcenreservierung für Geräte, GPU, RAM und APIs
PAR-05	Priorisierung ohne dauerhaftes Verhungern niedriger Prioritäten
PAR-06	Ereignis- und zeitbasierte Aktivierung
PAR-07	Abbruch und Timeout einzelner Tasks
PAR-08	Fehlerisolation pro Aufgabe und Adapter
PAR-09	Konsistente gleichzeitige Artifact-Schreibvorgänge
PAR-10	Wiederaufnahme nach Abstürzen ohne doppelte externe Wirkung
PAR-11	Backpressure und Lastbegrenzung
PAR-12	Messbare Fairness, Durchsatz und Latenz

7.3. Ein zentrales technisches Problem: Gleichzeitige Änderungen
Der heutige Store besitzt bereits SQLite-Transaktionen und eindeutige Versionen.

Das ist eine gute Grundlage, reicht aber nicht automatisch für beliebig viele nebenläufige Aufgaben und verteilte Worker aus.

Ich würde zusätzlich definieren:

Optimistische Zustandsänderungen: Ein Task darf einen aktuellen Zustand nur aktualisieren, wenn dessen erwartete Revision noch gilt.

Atomare Zustandsübergänge: Task-Status, Artefaktreferenzen und Freigabeinformationen müssen konsistent gespeichert werden.

Idempotente Auftrags-IDs: Wiederholte Ausführung desselben Auftrags darf keine ungewollte zweite Wirkung auslösen.

Leases für laufende Arbeiten: Ein Worker erhält einen zeitlich begrenzten Ausführungsanspruch; nach Ausfall kann ein anderer übernehmen.

Exklusive Ressourcen: Zwei Tasks dürfen nicht gleichzeitig widersprüchliche Steuerbefehle an dieselbe Drohne senden.

Für physische Aktoren genügt ein Lease allein nicht. Auch der Adapter beziehungsweise Gerätecontroller muss veraltete oder konkurrierende Aufträge erkennen können.
7.4. Wo entsteht die neue Implementierung?
Der heutige synchrone Schwerpunkt liegt in apps/adapter-host/src/task_runtime/mod.rs, Zeilen 74–169.

Die langfristige Erweiterung könnte getrennte Funktionen für Scheduling, Job-Dispatch, Worker-Ergebnisse und Ressourcenzugriffe erhalten.

Beispielsweise als vorgeschlagene Struktur:
apps/adapter-host/src/task_runtime/
    scheduler.rs
    dispatch.rs
    execution.rs
    recovery.rs
    resource_leases.rs
    goal/


Auch diese Struktur wäre keine zusätzliche übergeordnete Instanz.

Es sind interne Zuständigkeiten derselben Runtime.

8. Langzeitautonomie – Aufgaben über Monate
Für Langzeitaufgaben würde ich drei Dinge strikt unterscheiden:

Ziel: Was soll erreicht werden?

Plan: Wie könnte das Ziel erreicht werden?

Ausführungszustand: Was wurde tatsächlich versucht, bestätigt, unterbrochen oder abgeschlossen?

Diese Zustände sollten miteinander verbunden, aber nicht identisch sein.

Ein Plan darf sich ändern, ohne dass das ursprüngliche Ziel verloren geht. Ein fehlgeschlagener Schritt darf ersetzt werden, ohne die gesamte Projekthistorie zu überschreiben.
8.1. Zukünftige Task-Zustände
Zustand	Bedeutung
created	Ziel wurde angenommen
ready	Voraussetzungen erfüllt, ausführbar
running	Aktiver Arbeitsschritt
waiting_event	Wartet auf ein externes Ereignis
waiting_time	Wartet bis zu einem geplanten Zeitpunkt
waiting_dependency	Andere Teilaufgaben müssen zuerst enden
awaiting_approval	Menschliche oder autorisierte Freigabe erforderlich
blocked	Keine zulässige Fortsetzung möglich
failed	Ein Arbeitsschritt ist fehlgeschlagen
completed	Erfolg anhand definierter Kriterien bestätigt
cancelled	Aufgabe wurde beendet

Die bestehende Goal-Runtime kennt bereits einige dieser Zustände. Das hier ist das erweiterte Zieldatenmodell.
8.2. Dauerhafte Aufgabenanforderungen
LNG-01: Aufgaben über Neustarts und Host-Ausfälle erhalten.

LNG-02: Teilziele, Abhängigkeiten und Fortschritt versioniert speichern.

LNG-03: Auf Ereignisse reagieren können, ohne permanent einen Reasoner laufen zu lassen.

LNG-04: Zeitpunkte und wiederkehrende Prüfungen persistent verwalten.

LNG-05: Aufgaben unterbrechen und später mit geeignetem Erinnerungskontext fortsetzen.

LNG-06: Eigene Budgets für Zeit, Tokens, Geld, API-Aufrufe und externe Wirkungen.

LNG-07: Ziele nach veränderten Voraussetzungen neu bewerten.

LNG-08: Entscheidungen, die menschliche Zustimmung benötigen, sichtbar zur Prüfung anbieten.

LNG-09: Einen nachweisbaren Abschluss statt bloß behaupteten Erfolg speichern.

LNG-10: Weiterarbeit auch nach längeren Zeiträumen ohne aktive Benutzersitzung ermöglichen – sofern der ARCS-Host beziehungsweise die erforderliche Infrastruktur betrieben wird.
8.3. Was passiert während mehrmonatiger Aufgaben?
Ein Beispiel:

Du beauftragst ARCS mit der Entwicklung einer Software für deine Drohne.

Am ersten Tag erstellt ARCS einen Plan und testet den ersten Prototyp.

Danach kann der Task auf den nächsten Test, einen Termin oder eine externe Antwort warten. Der aktuelle Zustand bleibt gespeichert.

Einige Tage später wird die Aufgabe wieder aktiv. ARCS lädt die relevanten Artefakte, bewertet den Fortschritt und arbeitet weiter.

Auch nach einem Neustart muss dieser Zusammenhang erhalten bleiben.

Das Artifact-Netz ist dabei nicht nur Erinnerung an das Projekt. Es ist die dauerhafte Informationsgrundlage, auf der der nächste Schritt aufbaut.

9. Intelligentes Capability- und Reasoning-Routing
Die aktuell vorhandenen Capability-Verträge beschreiben bereits Rolle, akzeptierte Schemas und ausgegebene Schemas.

Das ist technisch wertvoll, aber noch keine vollständige Beschreibung einer Fähigkeit.

Ein universeller Agent benötigt zusätzlich Informationen über den tatsächlichen Einsatzbereich.
9.1. Erweiterter Capability-Vertrag
Ein zukünftiger Vertrag sollte mindestens ausdrücken können:
Feld	Bedeutung
Identity	Stabile Identität der Fähigkeit
Role	Input, Observe, Request, Reason, Act, Output oder Transform
Purpose	Wofür die Fähigkeit geeignet ist
Inputs / Outputs	Akzeptierte und erzeugte Datenverträge
Preconditions	Welche Voraussetzungen gelten
Effects	Welche Zustandsänderungen möglich sind
Permissions	Welche Berechtigungen erforderlich sind
Constraints	Fachliche und technische Grenzen
Resources	Benötigte Rechenleistung, Speicher, API-Kosten oder Geräte
Availability	Erreichbarkeit und aktueller Betriebszustand
Reliability	Messbare bisherige Erfolgs- und Fehlerstatistik
Version	Implementierungs- und Vertragsversion

Wichtig ist, die Selbstbeschreibung eines Adapters nicht mit einer verifizierten Fähigkeit gleichzusetzen.

Ein Adapter kann behaupten, dass er eine bestimmte Operation ausführt. ARCS muss anschließend durch Tests, Verträge und tatsächliche Ergebnisse prüfen können, wie zuverlässig das funktioniert.
9.2. Dynamische Auswahl
Der Prozess sollte so aussehen:
Ziel und Teilaufgabe

Was muss erreicht werden?

Fähigkeiten suchen

Welche Capabilities könnten helfen?

Voraussetzungen prüfen

Welche sind geeignet, verfügbar und erlaubt?

Alternativen bewerten

Qualität, Kosten, Zeit und Risiko

Capability auswählen

Ein oder mehrere Verfahren beauftragen

Ergebnis prüfen

Verifikation und gemessene Wirkung

Erfahrung speichern

Erkenntnisse im Artifact-Netz aktualisieren


9.3. Reason Manager
Der Reason Manager soll nicht grundsätzlich jedes Problem an ein LLM schicken.

Er soll unter mehreren Verfahren wählen können:
- Ein mathematischer Solver für klar definierte Optimierungsprobleme.
- Ein Entscheidungsmodell wie Jev für geeignete strukturierte Entscheidungen.
- Ein LLM für offene Planung, Sprache, Code und komplexe Interpretationsaufgaben.
- Ein regelbasiertes Verfahren für bekannte, deterministische Abläufe.
- Mehrere Verfahren parallel, wenn ein Ergebnisvergleich sinnvoll ist.
Für jede Methode sollen gemessene Erfolgsraten und Ressourcenanforderungen bekannt werden.

Die Auswahl könnte langfristig selbst ein Optimierungsproblem sein:
\[
\pi^*=\arg\max_\pi
\mathbb{E}\left[
U_{\mathrm{task}}
-\lambda_c C
-\lambda_t T
-\lambda_r R
\right]
\]

Dabei beschreibt \(U_{\mathrm{task}}\) den Aufgabennutzen, \(C\) die Kosten, \(T\) die Zeit und \(R\) das Risiko.

Die Gewichte müssen konfigurierbar sein und dürfen harte Sicherheitsregeln nicht ersetzen.

10. Sicherheit und Kontrolle in der Heterarchie
Heterarchie darf nicht zu unkontrollierter Kommunikation und Ausführung führen.

Daher würde ich die Architektur in einem Satz charakterisieren:

Verteilte Problemlösung bei konsistenter Autorität.

Das heißt:
- Reasoner können Pläne und Alternativen entwickeln.
- Manager können unabhängige Operationen verarbeiten.
- Verschiedene Komponenten können kooperieren.
- Die erforderlichen Rechte werden aber nicht durch Kooperation oder Delegation erweitert.
10.1. Zwingende Sicherheitsanforderungen
ID	Anforderung
SEC-01	Jeder Adapter besitzt eine authentifizierte Identität
SEC-02	Manifest und Betreiber-Grant bleiben getrennt
SEC-03	Jede Action und jeder kontrollierte Output wird vor Ausführung autorisiert
SEC-04	Freigaben gelten nur für die konkrete Version einer Handlung
SEC-05	API-Schlüssel sind auf die tatsächlich berechtigten Prozesse begrenzt
SEC-06	Generierter oder fremder Code wird isoliert ausgeführt
SEC-07	Ressourcen- und Zugriffslimits sind verbindlich
SEC-08	Unsichere physische Aktionen werden geräteseitig zusätzlich begrenzt
SEC-09	Fehler, Ablehnungen und Freigaben werden nachvollziehbar gespeichert
SEC-10	Kein unkontrollierter Autoritätszuwachs durch Delegation

Eine konkrete aktuelle Baustelle ist die Weitergabe von Modellschlüsseln: In apps/control-panel/arcs_panel/controller.py, Zeilen 150–159, wird die Prozessumgebung an den Host weitergegeben. In apps/adapter-host/src/runtime.rs, Zeilen 417–425, werden Adapterprozesse ohne ausdrücklich getrennte Umgebungen gestartet.

Bei fremden Adaptern sollte dieses Verhalten vor dem produktiven Einsatz abgesichert werden, damit nicht jeder Kindprozess Zugangsdaten erbt, die nur ein bestimmter Reasoner benötigt.

11. Konkrete zukünftige Implementierungsstruktur
Ich würde die bisherige Architektur erweitern und ordnen, nicht durch ein neues Framework ersetzen.

Die folgende Struktur ist ein Vorschlag, welche Zuständigkeiten langfristig wohin gehören könnten.
ARCS/
├── crates/
│   ├── arcs-core/
│   │   └── src/
│   │       ├── core/               # Artifact- und Schema-Grundverträge
│   │       ├── store/
│   │       │   ├── database/       # Persistenz
│   │       │   ├── network/        # Assoziative Aktivierung
│   │       │   ├── relations/      # Typisierte Verknüpfungen
│   │       │   ├── retrieval/      # NEU: Gedächtnisabruf
│   │       │   └── consolidation/  # NEU: Erfahrungskonsolidierung
│   │       ├── world/              # Weltmodell
│   │       ├── reasoning/          # Reasoning-Verträge und Routing
│   │       ├── decision/           # Verification und Approval
│   │       ├── adapters/           # Registrierung, Grants, Ports
│   │       └── runtime/
│   │           ├── goals/          # NEU: Langfristige Zielverwaltung
│   │           ├── planning/       # NEU: Planartefakte und Abhängigkeiten
│   │           ├── coordination/   # NEU: Heterarchische Kooperation
│   │           └── agent_cycle/    # Vorhandene Agentenzyklus-Fassade
│   ├── arcs-adapter-protocol/
│   └── arcs-adapter-sdk/
│
├── apps/
│   ├── adapter-host/
│   │   └── src/
│   │       ├── managers/           # NEU: Sechs Rollen-Manager
│   │       ├── task_runtime/
│   │       │   ├── scheduler.rs    # NEU
│   │       │   ├── dispatch.rs     # NEU
│   │       │   ├── recovery.rs     # NEU
│   │       │   ├── resources.rs    # NEU
│   │       │   └── goal/           # Vorhandene Goal-Runtime
│   │       └── runtime.rs          # Vorhandener Host
│   └── control-panel/
│
├── adapters/
│   └── ...                         # Austauschbare externe Adapter
└── docs/                            # Verträge und Architekturentscheidungen



Diese Struktur soll nicht eins zu eins als sofortiger Refactoring-Auftrag verstanden werden.

Beispielsweise könnte sich während der Implementierung ergeben, dass die bestehende Goal-Runtime besser im Host bleibt und nur ihre allgemeinen Verträge im Core definiert werden.

Entscheidend sind die Verantwortlichkeiten, nicht möglichst viele neue Verzeichnisse.

12. Implementierungsplan mit Prioritäten und Abnahmekriterien
Ich würde die Aufgaben in folgende Entwicklungspakete einteilen.
Paket	Priorität	Wesentliche Arbeit	Abnahme
ARC-01 – Architekturvertrag	P0	Sternarchitektur, Managerzuständigkeiten, Grenzen dokumentieren	Bestehende Komponenten bleiben kompatibel
ARC-02 – Managerstruktur	P0	Sechs logisch getrennte Manager aus vorhandenem Host entwickeln	Multi-Rollen-Adapter funktionieren unverändert
MEM-01 – Artifact Retrieval	P0	Graphsuche, Kontextauswahl, Relevanzprüfung	Früheres Wissen wird gezielt gefunden
RSN-01 – Echte KI	P0	LLM und Entscheidungsmodell anschließen	Aufgaben mit echter Inferenz abschließen
LNG-01 – Dauerhafte Aufgaben	P0	Ziele, Wartezustände, Scheduler, Recovery	Aufgabe über Neustarts fortsetzen
HET-01 – Dynamische Planung	P0	Teilziele, Abhängigkeiten, Capability-Auswahl	Unbekannte Aufgabe ohne feste Route
PAR-01 – Parallele Runtime	P0	Concurrent Tasks, Worker, Ressourcenverwaltung	Mehrere Tasks ohne gegenseitiges Blockieren
MEM-02 – Konsolidierung	P1	Erfahrungen und Skills wiederverwenden	Wiederholte Aufgaben profitieren messbar
TOL-01 – Werkzeugentwicklung	P1	Sandbox, Code, Build, Test, Registrierung	Neues Werkzeug kontrolliert nutzbar
DRN-01 – Autonome Drohne	P1	Wahrnehmen, Suchen, Planen, Steuern	Zielobjekt autonom finden
UX-01 – Universelle Oberfläche	fortlaufend	Einrichtung, Fortschritt, Freigaben	Keine technische Konfiguration für Standardaufgaben
OPS-01 – Betrieb und Skalierung	fortlaufend	Monitoring, Isolation, Benchmarks	Stabiler und nachvollziehbarer Langzeitbetrieb

Was würde ich zuerst praktisch umsetzen?
Ich würde nicht alle zwölf Pakete gleichzeitig beginnen.

Der erste Meilenstein sollte die kleinste funktionsfähige Kombination aus vier Bereichen sein:

Erstens: Ein echter LLM-Reasoner bearbeitet eine Aufgabe.

Zweitens: Die Aufgabe wird mit ihrem bisherigen Kontext im Artifact-Netz gespeichert.

Drittens: Die Aufgabe kann unterbrochen und später fortgesetzt werden.

Viertens: Die vorhandenen Capabilities werden ohne zusätzliche gerätespezifische Core-Logik genutzt.

Danach sollte die Fähigkeit zur heterarchischen und parallelen Verarbeitung schrittweise integriert werden.

Die sechs Manager würde ich dabei als Architekturregel durchgehend berücksichtigen, aber nur dann sofort umbauen, wenn dadurch konkrete Funktionen einfacher oder zuverlässiger werden.

13. Verbindliche Systemtests
Die eigentliche Architektur muss nicht nur auf dem Papier funktionieren. Sie muss unterschiedliche Aufgaben unter denselben allgemeinen Regeln bearbeiten können.
TEST-01 – Autonome Drohnenmission

Eine simulierte Drohne erhält nur das Ziel, einen Gegenstand in einem zunächst unbekannten Raum zu finden.

Prüfung: Sensorintegration, Gedächtnis, Planung, Bewegungssteuerung, Fehlertoleranz und evidenzbasierte Zielbestätigung.

TEST-02 – Langfristige Softwareentwicklung

ARCS erhält ein Entwicklungsziel und arbeitet in mehreren getrennten Sitzungen daran.

Prüfung: Teilplanung, Werkzeugnutzung, Erfahrungsspeicherung, Recovery, Fehlerbehebung und verlässliche Testergebnisse.

TEST-03 – Heterarchische Parallelität

Mehrere Reasoner bearbeiten unabhängige Teilaufgaben, während externe Komponenten weitere Informationen bereitstellen.

Prüfung: Abhängigkeitsmanagement, parallele Ausführung, Konfliktauflösung, Ressourcenverteilung und Ergebniszusammenführung.

TEST-04 – Artifact-Gedächtnis

ARCS bearbeitet mehrere ähnliche Aufgaben. Dabei soll es frühere Fehler, erfolgreiche Verfahren und relevante Fakten wiederfinden.

Prüfung: Retrieval-Qualität, falsche Erinnerungen, Kontextkosten, Wiederverwendung und Auswirkungen auf die Erfolgsrate.

TEST-05 – Monatslange Aufgabe

Eine Aufgabe wird über simulierte und später reale Zeiträume mit Neustarts, externen Ereignissen und Wartephasen betrieben.

Prüfung: Zustandskonsistenz, Terminverarbeitung, Langzeitgedächtnis, Berechtigungen und korrektes Wiederaufnehmen.


Für die Aussagekraft würde ich die Tests jeweils mit reproduzierbaren Ausgangszuständen, mehreren Durchläufen und geeigneten Baselines ausführen.

Insbesondere beim Gedächtnis sind Ablationsvergleiche interessant:
- Kein Erinnerungskontext
- Einfacher chronologischer Kontext
- Artifact-Retrieval
- Artifact-Retrieval mit Assoziationen und Konsolidierung
So lässt sich untersuchen, ob die zusätzlichen Mechanismen tatsächlich einen Vorteil bringen.

14. Nichtfunktionale Anforderungen
Neben den Funktionen selbst braucht ARCS verbindliche Qualitätsanforderungen.
Eigenschaft	Langfristige Anforderung
Universalität	Neue Geräte und Programme ohne gerätespezifische Core-Änderung anschließen
Modularität	Komponenten und Reasoner unabhängig ersetzen können
Parallelität	Mehrere Aufgaben und Operationen gleichzeitig betreiben
Persistenz	Relevante Zustände über Neustarts und längere Zeiträume erhalten
Nachvollziehbarkeit	Entscheidungen, Datenquellen und externe Wirkungen rekonstruieren können
Determinismus	Replays und Zustandsübergänge unter gleichen Bedingungen nachvollziehbar halten
Sicherheit	Berechtigungen und Freigaben auch bei Modellfehlern durchsetzen
Isolation	Fehler einzelner Adapter und Worker begrenzen
Skalierbarkeit	Wachsende Artefaktmengen und zusätzliche Rechner handhaben
Effizienz	Kontext, Tokens, Rechenzeit und Energie gezielt einsparen
Erweiterbarkeit	Neue Fähigkeiten und Verfahren ohne zentrale Sonderlogik integrieren
Benutzerfreundlichkeit	Normale Nutzung ohne Verständnis interner Architektur
Wartbarkeit	Klare Modulgrenzen, versionierte Verträge und automatische Regressionstests

Die Skalierung auf mehrere Rechner muss nicht im ersten Schritt vollständig umgesetzt werden. Die Architektur sollte aber die Möglichkeit offenlassen, Adapter und Worker später unabhängig zu verteilen.

15. Architekturentscheidungen, die ich verbindlich festhalten würde
ID	Entscheidung	Begründung
ADR-001	Ein logischer ARCS-Core	Keine unnötige zusätzliche Kontrollhierarchie
ADR-002	Sechs spezialisierte Anschluss-Manager	Klare Verantwortlichkeiten und universelle Außenweltanbindung
ADR-003	Transform als eigenständige, frei nutzbare Rolle	Datenverarbeitung ohne verpflichtende zentrale Übersetzung
ADR-004	Gemeinsames Artifact-Netz als Gedächtnis	Keine getrennten widersprüchlichen Wissensbestände
ADR-005	Heterarchische Kooperation	Dynamische Zusammenarbeit ohne starre Funktionshierarchie
ADR-006	Verpflichtende Parallelität	Notwendig für Universalagenten und langfristige Aufgaben
ADR-007	Externe, austauschbare Reasoner	Keine Bindung an ein einzelnes KI-Verfahren
ADR-008	Kontrollierte externe Wirkungen	Autonomie bei konsistenter Sicherheit
ADR-009	Persistente Ziele und Pläne als Artefakte	Mehrmonatige Aufgaben und nachvollziehbare Fortschritte
ADR-010	Logischer Stern statt erzwungener zentraler Datenübertragung	Skalierbarkeit und geringer Kommunikationsaufwand
ADR-011	Fähigkeitsausbau über kontrollierte Werkzeuge	Erweiterbarkeit über den ursprünglichen Funktionsumfang hinaus
ADR-012	Benutzeroberfläche abstrahiert technische Interna	Einfacher Zugang trotz komplexer Architektur


16. Gesamtbewertung
Die bestehende Codebasis bringt bereits wesentliche Voraussetzungen mit: versionierte Artefakte, semantische und gewichtete Beziehungen, Adapter-Verträge, Provenienz, Verification, Approval und eine erste dauerhafte Goal-Runtime.

Die größten noch fehlenden Systemeigenschaften liegen in der intelligenten Nutzung dieser Grundlage:
- Ein Gedächtnis, das passende Erfahrungen nicht nur speichert, sondern gezielt abruft.
- Eine heterarchische Planung, die unbekannte Aufgaben in geeignete Teilaufgaben zerlegt.
- Eine wirklich parallele Runtime mit zuverlässiger Ressourcen- und Konfliktverwaltung.
- Langzeitautonomie mit Scheduling, Wartezuständen und Wiederaufnahme.
- Dynamische Capability- und Reasoner-Auswahl.
- Kontrollierter, selbstständiger Ausbau neuer Fähigkeiten.
Ich würde deshalb keine weitere große Kontrollschicht einführen, sondern das vorhandene System entlang dieser sechs Schwerpunkte ausbauen.

Die präziseste Zusammenfassung deiner Architektur ist:
ARCS ist ein universeller, persistenter Agent mit einem einzigen logischen Core, sechs spezialisierten Schnittstellen zur Außenwelt, einem assoziativen Artifact-Gedächtnis sowie heterarchischer und paralleler Aufgabenbearbeitung. Externe Reasoner und Werkzeuge übernehmen die spezialisierte Problemlösung; der Core gewährleistet gemeinsame Zustände, Koordination, Nachvollziehbarkeit und kontrollierte Ausführung.

Das ist ein anspruchsvolles, aber technisch sinnvoll strukturiertes Zielbild. Die entscheidende Entwicklungsaufgabe besteht jetzt darin, diese Eigenschaften nicht nur als Konzepte zu definieren, sondern sie über dieselben allgemeinen Verträge in unterschiedlichen realen Anwendungsfällen nachzuweisen.