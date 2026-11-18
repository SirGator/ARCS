# Fähigkeitsbeschreibungen und kontrollierte Suche

Diese Erweiterung setzt einen Teil von `ARCHITECTURE.md`, v3 §9.1/§9.2 und
den v2-Aufgaben CAP-01/CAP-02 um. Die vorhandene Registry beschreibt und findet
freigeschaltete Fähigkeiten; eine zweite zentrale Werkzeugverwaltung entsteht
nicht. Der Planer erhält zusätzliche Beschreibungen seiner autorisierten
Vorlagen. Ihre Selbstbeschreibung bleibt von Berechtigungen und Nachweisen
tatsächlicher Wirkung getrennt.

## Beispiel

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/discovery/host.json \
  --find-capabilities apps/adapter-host/config/discovery/query.json
```

Der Host verbindet einen Input-Adapter, einen deterministischen Solver und
einen Modell-Testadapter. Die Anfrage sucht nach `nearest`, Rolle `reason`,
passendem Candidate-Schema und freigegebener Methode `solver`. Sie liefert nur
`example.solver / solution.propose`. Der Modell-Testadapter führt keine echte
Inferenz aus. Die Suche pollt keine Eingaben, startet kein Reasoning und führt
keine Handlungen aus. Verbindungen werden regulär beendet. Die Daten liegen
getrennt unter `var/discovery/`; es wird kein Runtime-Journal angelegt.

## Gemeinsamer Adaptervertrag

Eine Capability im Manifest kann optional `description` enthalten:

```json
{
  "id": "solution.propose",
  "contract": {
    "kind": "reason",
    "emits": ["arcs.solution_candidate.nearest.v1"]
  },
  "required_permissions": ["reason.compute"],
  "description": {
    "purpose": "Find the nearest listed numerical value",
    "tags": ["nearest", "numeric"],
    "preconditions": ["Input contains finite values and a target"],
    "effects": ["Returns a candidate"],
    "constraints": ["At most 4096 values"],
    "resources": ["Local CPU"]
  }
}
```

`purpose` muss 1–1024 Bytes enthalten. Bis zu 32 eindeutige Tags mit je
1–64 ASCII-Buchstaben, Ziffern, `.`, `_` oder `-` sind zulässig. Die vier
weiteren Listen haben jeweils höchstens 16 eindeutige, nichtleere Einträge mit
je 256 Bytes. Kontrollzeichen sind unzulässig. Die gesamte serialisierte
Beschreibung ist auf 8192 Bytes begrenzt. Unbekannte Felder, beispielsweise
ein selbst gesetztes `verified`, werden abgewiesen.

Rollen und Ein-/Ausgabeschemas bleiben im technischen `contract`, erteilte
Rechte bleiben im Betreiber-Grant. Eine Aussage wie „keine Nebenwirkungen“
hebt weder den Act-Vertrag noch Verification, Approval oder Idempotenz auf.
`resources` beschreibt nur eine Behauptung; die verbindliche Zulassung erfolgt
über die vorhandenen Grants und [Kapazitätspools](resource-pools.md).

Katalog, Suchtreffer und Planungskontext kennzeichnen Beschreibungen als
`description_claim` mit `source: "adapter"` und `verification: "unverified"`.
Auch ein mit hohem Trust oder als System installierter Adapter liefert damit
keinen semantischen Wirkungsnachweis. Die Methode einer Reason-Capability
wird aus dem Betreiber-Grant übernommen und beweist noch keine fachliche
Eignung der Implementierung.

## Suchvertrag

`CapabilityQuery` unterstützt folgende optionale Felder:

| Feld | Bedeutung |
| --- | --- |
| `text` | Lexikalische Suchwörter, höchstens 4096 Bytes und 32 verschiedene Wörter |
| `role` | Eine der sieben vorhandenen Adapterrollen |
| `accepts_schema` | Technisch akzeptiertes Schema |
| `emits_schema` | Technisch erzeugtes Schema |
| `reasoning_method` | Für Reason freigegebene Methode |
| `allowed_capabilities` | Expliziter zusätzlicher Suchbereich, höchstens 256 eindeutige Referenzen |
| `limit` | 1–64 Treffer; Standard 20 |

Alle Suchwörter müssen vorkommen. Die Suche zerlegt Text an nicht
alphanumerischen Zeichen und ignoriert Groß-/Kleinschreibung. Sie bewertet
Treffer je Wort mit 4 Punkten für Tags, 2 für den Zweck und 1 für Identität
oder weitere Beschreibungsfelder. Bei Gleichstand entscheiden Adapter- und
Capability-ID. Ein leerer Suchtext erlaubt eine reine Vertragsabfrage. Alte
Adapter ohne Beschreibung bleiben anhand ihrer Identität und Verträge
auffindbar; fehlende Semantik wird nicht erfunden.

Ein fehlender Suchbereich bedeutet den freigeschalteten Katalog. Eine explizit
leere Liste bedeutet keine Treffer. Unbekannte oder nicht freigeschaltete
Referenzen im Suchbereich sind Fehler. Ungültige Suchanfragen werden nicht als
unbeschränkte Suche oder Modell-Fallback interpretiert. Die CLI prüft JSON,
Suchregeln und eine Dateigrenze von 65536 Bytes vor dem Core- oder Adapterstart.
`--find-capabilities` lässt sich nicht mit Runtime-, Monitor- oder Aktionsbefehlen
kombinieren.

Der Core stellt `AdapterRegistry::find_capabilities` bereit.
`Host::find_capabilities` begrenzt zusätzlich auf angeschlossene externe
Installationen und schließt bereits als nicht verfügbar markierte Adapter aus.
Diese Filter gelten vor der Ergebnisbegrenzung, damit ein gesperrter Treffer
keine verfügbare Alternative verdeckt. Das ist kein frischer Healthcheck
und keine Reservierung von Adapter-, Modell- oder Gerätekapazität. Jeder spätere
Manager-Aufruf muss erneut Autorisierung und Ressourcen prüfen.

## Planer, Gedächtnis und Wiederaufnahme

Der bestehende `task_planning.templates`-Katalog ergänzt bei beschriebenen
Vorlagen `reasoner_description_claim`. Bei Goal-Vorlagen werden beschriebene
autorisierte Aktionen unter `action_description_claims` aufgeführt. Andere
Capabilities und nicht freigegebene Fähigkeiten gelangen nicht in diesen
Vorlagenkontext.

Die Angaben werden als Daten in der bestehenden Reasoning-Anfrage an den
externen Planer übergeben. Er darf weiterhin nur Betreiber-Vorlagen auswählen.
Die Strukturprüfung des Plans, Budgetgrenzen, zulässige Exporte, Grants und
manuelle Goal-Freigaben bleiben verbindlich. Selbst ein feindseliger
Beschreibungstext kann keine neue Capability oder Vorlage installieren.

Die Beschreibungen gehören zum Manifest und damit zur gebundenen
Runtime-Definition. Der Planungskontext bleibt über eine verlorene Antwort und
einen Neustart hinweg gleich. Eine geänderte Beschreibung kann nicht still
unter demselben Runtime-Journal weiterverwendet werden. Bereits laufende
Worker-Verfügbarkeit wird deshalb nicht in diesen statischen Katalog gemischt.
Die vorhandenen Grenzen für Manifest-, Definitions- und Anfragegrößen gelten
zusätzlich zur einzelnen Beschreibungsgrenze.

## Kompatibilität und offene Arbeit

Ohne Beschreibung bleiben serialisierte Manifeste und bestehende
Planungsanfragen unverändert. Neue Beschreibungsfelder benötigen einen
aktualisierten Host; ältere Hosts können diese Erweiterung ablehnen.
Externe Rust-Adapter müssen in `CapabilityDescriptor`-Literalen das neue Feld
`description: None` oder eine Beschreibung angeben. Die lokalen Adapter im
ARCS-Workspace sowie die vorhandenen Drohnen-/Modell-Anbindungen sind angepasst.
Das Beispielprogramm `arcs-example-reasoner` übernimmt eine optionale
Beschreibung aus seinem Profil; bei Änderung wird ein neues Journal benötigt.

Die Suche ist lexikalisch. Sie erzeugt keine Synonyme, semantischen Embeddings,
Erfolgswahrscheinlichkeiten oder Kostenbelege. Voraussetzungen und Wirkungen
werden durch die Suche nicht ausgewertet oder bestätigt. Vollständig autonomes
Reasoner-Routing, Verfügbarkeitssnapshots für Neuplanung, Fähigkeitsprüfungen,
gemessene Zuverlässigkeit und freie Werkzeugkomposition bleiben offen.

## Prüfung

```bash
cargo test -p arcs-adapter-protocol --lib --locked --offline
cargo test -p arcs-core --lib adapters::registration::discovery --locked --offline
cargo test -p arcs-host --test capability_discovery --locked --offline
```

Zwei Protokoll-, vier Core- und sechs Prozess-Tests prüfen Grenzen und
Abwärtskompatibilität, lexikalische Relevanz, harte Filter, fehlende Grants,
bekannte Ausfälle, CLI ohne Task-Ausführung, eine beschreibungsabhängige
Vorlagenauswahl, Wiederaufnahme, Definitionsdrift und feindselige Behauptungen.
