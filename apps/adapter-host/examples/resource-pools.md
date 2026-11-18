# Gemeinsame Kapazitätspools für Reasoning

Dieser Schritt ergänzt `ARCHITECTURE.md`, v3 §7.2 PAR-04/PAR-11 und §7.3:
Ein Host begrenzt Reasoning-Aufrufe verschiedener Adapter anhand gemeinsamer
benannter Kapazitäten. Mehrere Runtimes innerhalb dieses Hosts verwenden
dieselben Grenzen. Das ergänzt den bestehenden Worker-Pool und die exklusiven
Adapter-Sessions.

Die Kapazitäten steuern die **Zulassung logischer Aufrufe innerhalb eines
Host-Prozesses**. Sie sind noch keine systemweiten Geräte-Leases, GPU- oder
RAM-Hardlimits und keine zeitbasierten API-Ratenlimits.

## Startbares Beispiel

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/resource-pools/host.json \
  --runtime apps/adapter-host/config/resource-pools/runtime.json \
  --ticks 12
```

Das Beispiel verwendet den vorhandenen Solver und den aufgezeichneten
Modell-Testadapter. Beide brauchen einen `analysis_slots`-Platz. Obwohl zwei
Worker erlaubt sind, wird daher nur einer dieser Aufrufe gleichzeitig
ausgeführt. Die beiden Ergebnisse erreichen `ready_for_review`.
Die Journale liegen getrennt unter `var/resource-pools/`.

Eine Zustandsausgabe einschließlich Poolbelegung liefert der Monitor:

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/resource-pools/host.json \
  --runtime apps/adapter-host/config/resource-pools/runtime.json \
  --monitor --stop-file /tmp/arcs-resource-pools.stop --ticks 12
```

Die Stop-Datei darf vor dem Monitorstart nicht existieren. Das kleine Beispiel
beendet seine Berechnungen schnell; eine Momentaufnahme muss daher keine
aktive Belegung zeigen. Die Prozess-Tests halten Aufrufe gezielt offen, um die
gleichzeitigen Belegungen und Wartezustände nachzuweisen.

## Betreiberkonfiguration

Die Host-Konfiguration definiert Kapazitäten. Jeder beteiligte Adapter kann
angeben, wie viele Einheiten er während eines Reasoning-Aufrufs benötigt:

```json
{
  "resource_pools": {"gpu.shared": 1, "ram_mb": 8192},
  "adapters": [
    {"program": "…", "grant_file": "model-a-grant.json",
     "reasoning_resources": {"gpu.shared": 1, "ram_mb": 4096}},
    {"program": "…", "grant_file": "model-b-grant.json",
     "reasoning_resources": {"gpu.shared": 1, "ram_mb": 2048}}
  ]
}
```

Das ist ein Strukturausschnitt; die vollständige lauffähige Konfiguration steht
in [`host.json`](../config/resource-pools/host.json). Die Namen sind frei
wählbare Betreiberbezeichner. Ein Name wie `ram_mb` legt nur die vereinbarte
Einheit fest und misst oder erzwingt keinen tatsächlichen Speicherverbrauch.
`gpu.shared` kann mehrere Modelladapter derselben GPU zuordnen.

- Höchstens 64 Pools, jeweils mit 1–1.000.000.000 ganzzahligen Einheiten.
- Namen mit 1–64 ASCII-Buchstaben, Ziffern, `.`, `_` oder `-`.
- Höchstens 64 Anforderungen pro Adapter; jede muss positiv sein und einzeln
  in den zugehörigen deklarierten Pool passen.
- Ohne `resource_pools` beziehungsweise `reasoning_resources` bleiben diese
  Konfigurationsteile leer. Bestehende Konfigurationen benötigen keine Änderung.

Ungültige Deklarationen werden vor dem Öffnen der Core-Datenbank und vor dem
Start externer Prozesse abgewiesen. Die Anforderungen kommen ausschließlich
aus der Host-Konfiguration. Ein Planer kann sie nicht in seinem Vorschlag
überschreiben. Grants, Verification und Approval bleiben unabhängig davon gültig.

## Zulassung und Freigabe

Ein neuer asynchroner Aufruf muss gleichzeitig Platz im Worker-Pool, seine benötigten
Adapter-Sessions und sämtliche benannten Kapazitäten erhalten. Es gibt keine
Teilreservierung wartender Aufrufe. So kann ein auf die GPU wartender Aufruf
nicht bereits den gesamten RAM-Pool blockieren.

Für binäre Eingaben werden die Anforderungen aller beteiligten Datenquellen
und des Reasoners addiert. Ein Adapter zählt pro Aufruf einmal, auch wenn er
mehrfach beteiligt ist. Übersteigt diese Summe bereits die Gesamtkapazität,
ist die Operation nicht ausführbar und wird abgewiesen. Vorübergehend belegte
Kapazität führt dagegen zu einem Wartezustand. Andere zulässige Aufgaben können
weiterlaufen; die Ressourcenwartezeit verbraucht keinen Reasoning-Worker.

Die Belegung wird direkt aus den ausstehenden Worker-Aufrufen abgeleitet.
Dadurch gibt es kein zweites veränderliches Reservierungsregister. Wenn der
Host eine Antwort übernimmt oder einen beendeten Aufruf entfernt, wird dessen
Kapazität frei. Das gilt auch nach ungültigen Antworten, Fehlern und Timeouts.
Alle Runtimes eines gemeinsamen Hosts müssen weiter bedient werden, damit ihre
Antworten und Deadlines verarbeitet werden.

Ein Planabbruch gibt ausstehende Worker-Kapazität nicht vorzeitig frei.
Abgebrochene Aufrufe behalten ihre Einheiten bis zur Antwortübernahme oder
zum Abbruch an ihrer Deadline. Rechtzeitige Antworten werden verworfen;
unabhängige Adapter dürfen die anschließend freien Einheiten nutzen.

Synchrone Reasoning-Aufrufe berücksichtigen laufende asynchrone Belegungen.
Bei fehlender Kapazität liefert der direkte Host-Port `HostError::ResourceBusy`.
Die Task-Runtime behandelt dies als vorübergehendes Warten, auch im synchronen
Modus. Der exklusive veränderliche Host-Zugriff verhindert während des
synchronen Aufrufs weitere neue Dispatches desselben Hosts.

## Wiederverwendung und Monitoring

Bereits im Core gespeicherte Ergebnisse benötigen keine neue Rechenkapazität.
Sie können auch bei vollen Pools und Workerplätzen übernommen werden. Der
Core prüft dabei weiterhin den vollständigen Request, seine Eingaben, aktuelle
Grants und die gespeicherten Artefakte. Dieselbe Request-ID mit veränderter
Anfrage wird weiterhin abgewiesen; die Cache-Abfrage ist keine Berechtigung.

`Host::resource_usage()` und `HostReport.resource_pools` liefern pro Pool:
`name`, `capacity`, `used` und `available`. Monitor-Snapshots enthalten dieselben
Angaben unter `resource_pools`. `TickReport.waiting_resources` meldet die in
diesem Tick besuchten, wegen benannter Kapazität wartenden Aufträge mit
`sequence`, `route_id`, `resource`, `requested` und `available`. Sie werden nicht
als fehlgeschlagene Aufgaben gezählt. Das ist keine vollständige historische
Liste aller wartenden Jobs; begrenzte Tick-Seiten und die bestehende Umlauf-
Reihenfolge bleiben wirksam.

## Wiederaufnahme und Grenzen

Nach einem Host-Neustart entstehen Kapazitätsbelegungen beim erneuten
Dispatch. Stabile Invocation-IDs, eingefrorene Kontexte und Backend-Replay
bleiben unverändert. Ein bereits berechnetes Ergebnis wird weiterhin
wiederverwendet. Es wird keine persistente physische Reservierung behauptet:
Nach einem harten Absturz können externe oder entfernte Berechnungen noch
laufen. Adapter müssen weiterhin ihren Idempotenzvertrag erfüllen.

Diese Erweiterung koordiniert weder mehrere Host-Prozesse noch mehrere Rechner
oder fremde Programme. Direkte Request-, Act-, Transform-, Output- und
Datenlese-Ports erhalten dadurch keine allgemeinen Ressourcen-Leases; erfasst
werden die über Reason V2 beauftragten Berechnungen und deren Datenquellen.
Eine Bezeichnung wie `drone` würde keine exklusive Gerätesteuerung absichern.
Hierfür bleiben persistente Koordination und die Abweisung veralteter Befehle
im Gerätecontroller notwendig. Allgemeine Prioritäten, garantierte globale
Fairness, automatische Bedarfserkennung und harte Betriebssystemlimits sind
weitere Arbeit.

## Prüfung

```bash
cargo test --locked --offline -p arcs-host --test resource_pools
```

Die neun Prozess-Tests prüfen gemeinsam genutzte und mengenbasierte Kapazität,
Zulassung ohne Teilreservierung, unzulässige Anforderungen, Fehler, ungültige
Antworten, Timeouts, Abbruch, Wiederaufnahme, Cache-Replay bei belegten Pools
und die gemeinsame Nutzung durch eine parallele und eine synchrone Runtime.
