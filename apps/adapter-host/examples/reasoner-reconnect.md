# Automatische Wiederverbindung von Reasonern

Ein laufender Host kann eine ausgefallene reine Reason-Installation mit einer
ausdrücklichen Betreiberpolicy neu verbinden. Prozessstart und Metadaten-
Handshake laufen in begrenzten Hintergrund-Workern. Der Core übernimmt die
Session erst nach Prüfung und Speicherung ihres Verbindungsbelegs.

## Beispiel mit echtem Timeout und Prozessneustart

```bash
cargo build --workspace --bins --locked --offline
cargo run -p arcs-host --example reasoner-reconnect --locked --offline -- \
  apps/adapter-host/config/reasoner-reconnect/host.json \
  apps/adapter-host/config/reasoner-reconnect/runtime.json \
  var/reasoner-reconnect/solver.sqlite
```

Das Beispiel sperrt nach dem normalen Hoststart die separate SQLite-Datei des
Beispielsolvers. Sein erster Request überschreitet das Laufzeitbudget. Sobald
der Core den Fehler gespeichert hat, gibt das Beispiel die Datei frei. Die
Wiederverbindung startet den Solverprozess neu; ein eigener, ausdrücklich
erlaubter Folge-Request löst die Aufgabe. Ein unabhängiger Auftrag läuft
währenddessen weiter.

Die Ausgabe zeigt zwei `ready_for_review`-Ergebnisse, einen fehlgeschlagenen
und zwei gültige Reasoning-Versuche sowie `adapter_reconnections` mit
`state: "healthy"`, `generation: 1`, `attempt: 1`. Die erfolgreiche Recovery
verwendet wieder `example.solver`. Die Daten liegen getrennt unter
`var/reasoner-reconnect/`.

Ein normaler CLI-Neustart übernimmt dieselben Task-Ergebnisse:

```bash
./target/debug/arcs-host \
  --config apps/adapter-host/config/reasoner-reconnect/host.json \
  --runtime apps/adapter-host/config/reasoner-reconnect/runtime.json --ticks 3
./target/debug/arcs-host \
  --config apps/adapter-host/config/reasoner-reconnect/host.json \
  --reasoning-attempts apps/adapter-host/config/reasoner-reconnect/query.json
```

## Zwei getrennte Policies

Am Adapter im Host steht beispielsweise:

```json
{
  "reasoning_reconnect": {
    "max_attempts": 2,
    "delay_ms": 50,
    "handshake_timeout_ms": 1000
  }
}
```

Erlaubt sind 1 bis 8 automatische Verbindungsversuche je Störung, 1 bis 60000 ms
Verzögerung und ein eigener Handshake-Timeout von 1 bis 3600000 ms. Nach dem
Handshake gilt wieder der normale Adapter-`timeout_ms`. Die Reconnection
verändert keine Reasoning-Budgets.

Die [Route-Policy `reasoning_recovery`](reasoning-recovery.md) erlaubt unabhängig
davon neue logische Reasoning-Requests. Im Beispiel enthält sie einen weiteren
Eintrag für denselben Solver. Eine reparierte Verbindung startet keinen bereits
gescheiterten Task neu und setzt keine verbrauchten Versuche oder Freigaben
zurück. Neue, noch wartende Arbeit kann die reparierte Session verwenden.

## Ablauf und Grenzen

Der erste Hoststart benötigt weiterhin erfolgreiche Metadaten-Handshakes aller
konfigurierten Adapter. Die neue Funktion arbeitet im laufenden Host nach
einer bekannten Quarantäne. Sie startet keinen Überwachungs- oder Inferenz-
Request als Health-Probe. Ein unbeobachteter Ausfall einer bisher unbenutzten
Session kann deshalb erst bei deren nächster Verwendung auffallen.

Die Policy ist auf Installationen beschränkt, deren vollständiges Manifest
ausschließlich Reason-Capabilities enthält. Andere Rollen und gemischte
Geräteprozesse brauchen weiter eigene Wiederherstellungsregeln, bei physischen
Aktoren insbesondere Controller-Fencing. Es gibt höchstens 16 konfigurierte
Reconnect-Installationen und zwei gleichzeitige Handshake-Worker pro Host.
Ein zyklischer Umlauf berücksichtigt fällige Installationen; belegte benannte
Kapazitätspools und alte Endpoint-Nutzer verbrauchen keinen neuen Versuch.

Der Core speichert Definition und revisionierte Zustände als
`arcs.adapter_reconnect.v1` und `arcs.adapter_reconnect_state.v1` im bestehenden
Artifact-Netz. Zustände sind `healthy`, `waiting`, `connecting`, `exhausted`
und `rejected`. Der Versuch wird vor dem Prozessstart beansprucht. Erfolg,
Timeout, Transportfehler, Vertragsabweichung und Abbruch bleiben nachvollziehbar.
Die erste Fälligkeit wird nicht durch wiederholtes Polling verschoben.

Manifest und Grant müssen exakt zur gespeicherten Installation und aktuellen
Registry passen. Änderungen werden abgewiesen; die ursprüngliche Registry
wird nicht durch neue Claims ersetzt. `capabilities_verified: false` kennzeichnet,
dass der Beleg keine geprüfte Modellfähigkeit oder Ergebnisqualität bestätigt.
Ein erfolgreicher Metadaten-Handshake beendet die Störung. Ein späterer Ausfall
beginnt eine eigene Generation. Ein Host-Neustart kann eine neue Session erst
nach einem tatsächlichen erfolgreichen Startup-Handshake als gesund melden;
die alten Verbindungs- und Task-Versuche bleiben in der Historie.

Der Host prüft alte Endpoint-Nutzer vor dem Neustart. Er übernimmt die neue
Session erst nach dem Core-Commit. Bei einem fehlgeschlagenen Commit bleibt
das fertige Handshake-Ergebnis zurückgehalten; ein weiterer Prozessstart ist
dafür nicht nötig. Worker lesen oder schreiben keinen Core-Store. Normale
Aufgaben, Sensoren, Fristen und Abbrüche können während des Handshakes
fortschreiten.

Initialer Start und Wiederverbindung verwenden denselben Command-Builder und
dieselbe Umgebungs-Allowlist. Programmargumente und Umgebungswerte werden nicht
in die Verbindungsbelege kopiert. Ein lokaler Lock schützt automatische
Wiederverbindung für denselben kanonischen Core und `node_id` vor zwei lebenden
Besitzern. Unterschiedliche Hosts benötigen eigene Node-IDs; dies ist kein
verteilter Controller-Fencing-Vertrag.

`TaskRuntime` betreibt die Wartung zwischen Ticks, auch bei vollen oder pausierten
Task-Queues. Ein Monitor ohne TaskRuntime betreibt sie ebenfalls. Eingebettete
Host-Anwendungen können `poll_reconnections()` aufrufen. Shutdown bricht
unbeantwortete Handshakes ab und zählt den beanspruchten Versuch als
`interrupted`; ein gestoppter Host startet keine neuen Reconnect-Worker.
Auch beim Drop bleiben alte Nutzer bis zur Freigabe der Node-Besitzsperre
unter Kontrolle.

## Status und Prüfung

CLI und Monitor zeigen `adapter_reconnections`, die eingebettete API bietet
`Host::adapter_reconnections()`. Pro Installation stehen Core-Belegversion,
Definition, Generation, Versuch, Zustand, Fälligkeit und typisierte Fehlerklasse
zur Verfügung. `quarantined` und `handshake_in_progress` zeigen zusätzlich
den lokalen Hostzustand; letzteres umfasst auch einen noch ausstehenden
Core-Commit. Ein letzter gespeicherter Handshake ist keine kontinuierliche
Erreichbarkeitsgarantie.

```bash
cargo test -p arcs-core --lib --locked --offline adapters::reconnect
cargo test -p arcs-core --lib --locked --offline runtime::reservations
cargo test -p arcs-host --test reasoner_reconnect --locked --offline
```

Die Tests prüfen echte Prozessneustarts, Fehlerbudgets, aktuelle Grants,
unvertrauenswürdige Belege, konkurrierende Claims, atomaren Rollback, neue
wartende Arbeit, unveränderte gescheiterte Tasks, Metadatenabweichungen,
Ressourcenwarten, unabhängige Sensoren, Umgebungsfreigaben, RPC-Timeouts,
Core-Publikationsfehler, Shutdown und erneuten Hoststart. Wiederverbindung
anderer Rollen, Start mit zunächst unerreichbaren Installationen, semantische
Fähigkeitsprüfung und allgemeine Neuplanung bleiben weitere Arbeit.
