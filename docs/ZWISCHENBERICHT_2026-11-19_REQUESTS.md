# ARCS – Zwischenbericht zu parallelen Quellenabfragen, 19. November 2026

Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md), insbesondere
MGR-04/MGR-05, HET-03/HET-04, PAR-02/PAR-08/PAR-10/PAR-11 und LNG-04/LNG-05.
Dieser Slice beseitigt die synchrone Request-Grenze in der optional parallelen
Goal-Quellenbeschaffung. Die allgemeinen Phasenabnahmen bleiben offen.

## Umsetzung und Zuständigkeiten

Der bestehende `RequestService` besitzt jetzt getrennte `prepare`- und
`finish`-Operationen. Vorbereitung speichert die exakte Invocation vor der
externen Arbeit. Der opaque `PreparedRequest` bleibt beim Core-Aufrufer; Worker
erhalten nur die Request-Nachricht. Die bisherige synchrone `execute`-API
verwendet dieselben Operationen.

`finish` verwirft Antworten zu überholten Dispatch-Versionen oder geänderter
Grant-Grundlage. Die vorhandenen Prüfungen für erlaubte Capability, Schemata,
Korrelation, Referenz-/Payloadgrenzen, Herkunft und Trust gelten weiter.
Ergebnis und erfolgreicher Invocation-Zustand werden wie bisher atomar im
Artifact Store gespeichert. Es wurden keine neuen Modell- oder Gerätefälle
in den Core aufgenommen.

Der Host betreibt begrenzte Request-Worker mit begrenzten Antwortkanälen.
`max_concurrent_requests: 2..32` aktiviert sie für Goal-Quellen; Standard 1
bewahrt das bisherige synchrone Verhalten. Alle Runtimes desselben Hosts
zählen dieselben ausstehenden Request-Worker. Reasoning und Request besitzen
getrennte Arbeitsbudgets, teilen aber die exklusive Session jeder Installation.
Die neue Inferenzkonfiguration aktiviert beide Budgets mit jeweils 2.

Unabhängige Quellen desselben Goals werden ohne Warten auf die erste Antwort
angestoßen. Fertige Versionen werden einzeln festgehalten; ihre Reihenfolge
im endgültigen Kontext folgt der Konfiguration. Laufende oder auf Kapazität
wartende Queries verbrauchen keine Observe-Warteticks. Transport-Timeouts und
persistente Taskfristen bleiben wirksam.

Die Quellenbeschaffung liegt jetzt in `goal/acquisition.rs`; die eigentliche
Goal-State-Machine ist entsprechend kleiner. Gemeinsame Session-Zulassung
liegt unter `runtime/sessions.rs`, statt vom Reason-Worker-Modul abzuhängen.
`task_runtime/worker_lifecycle.rs` koordiniert die Übernahme beziehungsweise
Verwerfung bereits laufender Arbeit nach dauerhaften Taskänderungen. Es wird
keine übergeordnete Steuerungsinstanz eingeführt.

## Pause, Abbruch und Wiederaufnahme

Eine additive operative Journaltabelle bindet Request-Eingabeversionen an Task
und Goal-Runde. Sie enthält keine zweite Kopie von Quellenpayloads oder Fakten;
Ergebnisse und Invocation-Zustände bleiben im gemeinsamen Artifact-Netz.
Die Abfragen zur Bereinigung untersuchen nur die begrenzte Menge laufender
Worker, keine vollständigen Task- oder Rundenhistorien.

Pausierte Tasks können ausstehende Quellenantworten validieren, ohne weiter
zu planen oder Aktionen auszulösen. Beim Fortsetzen benötigen neue Runden
wie bisher neue Quellen, Verification und Freigaben. Ein dauerhaft abgebrochener,
abgelaufener oder durch eine neue Runde ersetzter Auftrag übernimmt keine
späte Quellenantwort. Vor dem erfolgreichen Kontroll-Commit wird nichts
verworfen; die Session bleibt bis Antwort oder Deadline belegt.

Nach verloren gegangener Antwort wird dieselbe persistierte Query mit derselben
Invocation-ID wiederaufgenommen. Der Prozessnachweis prüft dies gegen einen
Peer, der seine Antwort vor der simulierten Unterbrechung dauerhaft speichert.
Er erhält zwei Zustellversuche, aber genau einen gespeicherten Antwortdatensatz.
Ein weiterer Neustart nach Core-Übernahme sendet keinen neuen Auftrag.

`HostReport.request_workers` macht ausstehende beziehungsweise aufgegebene
Abfragen mit ihren Identitäten sichtbar. Quellenpayloads sind dort nicht enthalten. `retiring` kennzeichnet Threads,
deren Antwort bereits abgeholt wurde, die aber noch nicht vollständig beendet
sind. Auch sie belegen Session und Workerbudget. Shutdown und Drop warten auf
alle verbliebenen eigenen Worker, bevor die Host-Besitzsperre freigegeben wird.

## Prüfungen

Drei neue Core-Tests prüfen getrennte Vorbereitung ohne externen Aufruf,
korrekte Ergebnisübernahme mit Replay, überholte Dispatches und geänderte Grants.
Ein weiterer Unit-Test hält einen auslaufenden Worker an einem expliziten
Gate fest und prüft, dass er bis zu seinem tatsächlichen Ende gezählt wird.
Dreizehn neue Prozesstests prüfen:

- Eine blockierte Abfrage bei zwei weiterlaufenden Goals und Sensoren.
- Zwei gleichzeitig gestartete Quellen desselben Goals mit stabiler Reihenfolge.
- Backpressure bei vollem Pool und gemeinsame Grenzen zweier Runtimes.
- Fehlerhafte Quellenpayloads und Transport-Timeout ohne Wiederholung des Tasks.
- Weitergabe eines injizierten Core-Commitfehlers ohne teilweise veröffentlichte Quelle.
- Neustart nach bereits extern gespeicherter, noch nicht übernommener Antwort.
- Pause ohne Taskfortschritt und frische Quellen nach Resume.
- Abbruch mit fehlgeschlagenem Commit und danach verworfener später Antwort.
- Dauerhafte Fristen während einer laufenden Abfrage und gültige Konfigurationsgrenzen.
- Eine native Simulatoraktion erst nach manueller Testfreigabe und unveränderte
  Wirkungszahl nach Host-Neustart.

| Prüfung | Ergebnis |
| --- | --- |
| Übrige Rust-Workspace-Tests | 599 bestanden |
| Abschließende gesamte Request-Prozesssuite | 13 bestanden, 0 fehlgeschlagen |
| Workspace-Build mit allen Binärprogrammen und Beispielen | Bestanden |
| Formatprüfung und Clippy über alle Targets | Bestanden; nur zwei bestehende Hinweise in `reasoning/service.rs` |
| Monitor-Replay des bereits abgeschlossenen echten 14B-Laufs | Drei Ticks, unveränderte sechs Modellaufträge, genau zwei Wirkungen |

Im Gesamtlauf bestanden zunächst 611 Tests; der neue Commitfehlertest erwartete
noch einen direkten Storefehler. Die atomare Ergebnisübernahme meldet ihn
korrekt als `RequestError::Invocation(InvocationError::Store(_))`. Ausschließlich
diese Testassertion wurde auf den genauen Fehlerpfad präzisiert. Danach bestand
die gesamte Request-Prozesssuite mit allen 13 Tests. Zusammen mit den übrigen
599 bestandenen Tests sind damit alle 612 Workspace-Tests abgedeckt.
Die zusätzliche Monitor-Statusausgabe wurde anschließend mit Build, Clippy und
dem tatsächlichen CLI-Replay geprüft.

Der bereits abgeschlossene 14B-Auftrag wurde mit dem neuen Host und aktiviertem
Request-Budget erneut geöffnet. Drei Monitor-Ticks ließen beide SDK-Journale
bei zwei Planungs- und vier Assessment-Aufträgen. Der Simulator blieb aus mit
`actuation_count: 2`; Execution- und Outcome-Versionen blieben identisch.
Der Monitor zeigte keine ausstehenden Request-Worker und ein abgeschlossenes
Goal ohne Taskfehler.

Lokale Protokolle liegen unter `var/verification-requests/`.

## Grenzen

Die Änderung aktiviert asynchrone Request-Quellen von Goals und bietet die
öffentliche Polling-API. Bestehende synchrone Host-APIs sowie Act, Output und
Sensorpolling selbst bleiben synchron. Benannte GPU-/RAM-Ressourcen gelten
weiterhin für Reasoning; Request-spezifische API-Quoten sind nicht implementiert.
Verschiedene Runtime-Limits prüfen die aktuell gemeinsame Belegung und bilden
keine unabhängig reservierten Pools. Eine ausgefallene Mehrrolleninstallation
wird nicht automatisch neu verbunden.

Der nachgewiesene Adapter-Replay setzt einen dauerhaft deduplizierenden
Backend-Vertrag voraus. Der Hoststopp ist keine Rücknahme einer entfernten
Wirkung. Verteiltes Fencing, allgemeine Neuplanung, autonome Drohnenmissionen
und tatsächlicher Monatsbetrieb bleiben weitere Architekturabnahmen.
