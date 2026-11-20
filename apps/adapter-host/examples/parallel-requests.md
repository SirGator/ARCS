# Parallele Quellenabfragen in Goals

Die Goal-Runtime kann unabhängige Request-Quellen abfragen, während andere
Aufgaben, Reasoner und Sensoren weiterarbeiten. Die Antworten bleiben im
vorhandenen Core-Store; es gibt keinen zusätzlichen Manager oder Faktenbestand.

Die Runtime-Option lautet:

```json
{
  "max_concurrent_reasoning": 2,
  "max_concurrent_requests": 2
}
```

`max_concurrent_requests` begrenzt die gleichzeitig laufenden
Quellenabfragen auf 1–32. Mit dem Standardwert 1 wartet die betroffene
Aufgabe auf einen einzelnen Worker, während der Scheduler andere Aufgaben
bearbeitet. Höhere Werte erlauben mehrere gleichzeitige Quellenabfragen.
Das Limit kann bei gleicher Runtime-Definition angepasst werden.
[Inferenzkonfiguration](../config/live-llm/runtime.json)

Jeder neue Aufruf zählt die ausstehenden Request-Worker desselben Hosts,
einschließlich derjenigen anderer Runtimes und aufgegebener, noch laufender
Abfragen. Zwei Runtimes mit Limit 2 können somit gemeinsam höchstens zwei
solche Aufrufe belegen. Ein Host nimmt über diese Schnittstelle nie mehr als
32 ausstehende Request-Worker auf. Unterschiedliche konfigurierte Limits
sind Zulassungsgrenzen des jeweiligen Aufrufers, keine getrennten Pools.
Reasoning und Request besitzen getrennte Workerbudgets.

## Identität und Ergebnisübernahme

Der Core bereitet den Auftrag vor der externen Arbeit vor und speichert dessen
Dispatch. Nur die enge `RequestInvocation` geht zum Worker. Dieser hat weder
eine Datenbankverbindung noch Rechte, Artefakte zu veröffentlichen.

Bei der Rückkehr prüft der Core den genauen Dispatch, die unveränderte
Grant-Grundlage, Capability und Schemata, Korrelation, Referenzgröße und
Payloadgrenzen. Ergebnis und erfolgreicher Invocation-Zustand committen wie
bisher atomar. Ein älterer Dispatch darf eine spätere Wiederaufnahme nicht
abschließen. Das synchrone `RequestService::execute` verwendet dieselben
Vorbereitungs- und Abschlussprüfungen.

Bei mehreren Quellen werden unabhängige Abfragen bereits vor der ersten
Antwort gestartet. Fertige Versionen werden einzeln gespeichert. Erst wenn
alle Quellen vorliegen, entsteht der vollständige Planungskontext, in der
konfigurierten Quellenreihenfolge. Pending-Requests und fehlende Beobachtungen
sind unterschiedliche Zustände: Rechenplatzwarten und laufende Abfragen
verbrauchen keine `max_wait_ticks` für Observe-Ereignisse. Transport-Timeouts
und die maximale Lebensdauer des Tasks gelten weiterhin.

## Sessions, Pause und Neustart

Eine Adapterinstallation besitzt eine gemeinsame exklusive Session. Während
einer Abfrage verwenden Reason, Act, Input, Observe, Transform und Output
diese Installation nicht zusätzlich. Unabhängige Installationen bleiben
ansprechbar. Ein Protokoll-/Transportfehler beendet nur die betroffene Session;
ein fehlerhafter Quelleninhalt scheitert an der Core-Validierung.

Ein gespeicherter Quellenfehler beendet die betroffene Aufgabe mit
`failed`. Das gilt auch mit einem Request-Platz. Liegt der Fehler beim
Neustart bereits im Core, übernimmt die Runtime ihn in den Aufgabenstand.
Eine pausierte Aufgabe kann einen bereits laufenden Quellenabruf
validieren und dessen Ressourcen freigeben. Planung und Ausführung warten
bis zur Freigabe der Pause. Beim Fortsetzen gelten die frischen Runden und
Freigaben.

Nach dauerhaftem Abbruch, Ablauf oder einem Wechsel zu einer neuen Runde
werden zugehörige alte Antworten verworfen. Ein fehlgeschlagener
Abbruch-Commit verwirft keine noch gültige Antwort. Bis zur tatsächlichen
Antwort oder Deadline bleibt die Session belegt. `HostReport.request_workers`
und das gleichnamige Monitorfeld zeigen Invocation, Capability, Eingabeversion und den Verwerfungsstatus ohne
Quellenpayloads. `retiring` kennzeichnet bereits abgeholte, noch nicht vollständig
beendete Worker. Sie zählen weiterhin zum Budget und zur Session-Belegung.
Beim Hoststopp werden auch diese Threads vor Freigabe des Hostbesitzes beendet.

Ein Hoststopp bricht lokale Dialoge ab. Bei Wiederaufnahme verwendet eine
unfertige Quellenabfrage dieselbe Invocation-ID und dieselbe persistierte
Eingabe. Der externe Adapter muss dauerhaft deduplizieren, damit eine bereits
gespeicherte Antwort stabil bleibt. Eine schon im Core erfolgreiche Antwort
ist sofort abrufbar. Wenn der Core das Abfrageergebnis wegen eines
Speicherfehlers nicht veröffentlichen kann, bleibt der Dispatch für diese
Wiederaufnahme offen.

## Prüfen und Grenzen

```bash
cargo test -p arcs-core io::request --locked --offline
cargo test -p arcs-host --test parallel_requests --locked --offline -- --test-threads=1
```

Die Prozesstests verwenden explizite Dateigates und persistente Peer-Journale.
Sie prüfen unabhängige Goals, parallele Quellen desselben Goals, Sensorfortschritt,
Poolgrenzen, zwei Runtimes, fehlerhafte Daten, Timeout, verlorene Antworten,
Pause, Abbruch mit Rollback, Taskablauf und eine manuell freigegebene native
Simulatoraktion mit unveränderter Wirkung nach Neustart.

Diese Erweiterung betrifft Request-Quellen von Goals und die öffentliche
Polling-Schnittstelle `Host::poll_request_version`. `Host::request_version`
und konfigurierte einmalige Host-Abfragen bleiben synchrone APIs. Act, Output,
Input- und Observe-Polling werden dadurch ebenfalls nicht asynchron. Benannte
GPU-/RAM-Pools bleiben ein Reasoning-Budget; API-Quoten und Request-spezifische
benannte Ressourcen sind weitere Arbeiten. Automatisches Reconnect von
Request-/Mehrrolleninstallationen und verteiltes Controller-Fencing sind
weiterhin offen. Die vollständige allgemeine Parallelitätsabnahme bleibt offen.
