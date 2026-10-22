# Allgemeine Adapter zuerst

Die nächste Entwicklungsphase baut allgemeine Adapterfähigkeiten aus.
Anschließend wird versucht, mit diesen Fähigkeiten die simulierte Drohne
zu steuern. Diese Reihenfolge ist verbindlich; die Drohnensuche bleibt das
erste große Anwendungsziel.

## Vorhandene Grundlage

| Baustein | Stand |
| --- | --- |
| Gemeinsame IDs, Manifeste, Capability-Verträge und Nachrichten | `crates/arcs-adapter-protocol` |
| Grants, Schema-Prüfung und kontrollierte Ausführung | `crates/arcs-core` |
| Konfigurierbarer Host für mehrere externe Adapter | `apps/adapter-host` |
| Gemeinsame Adapter-Session mit Observe-, Request- und Act-Callbacks | `crates/arcs-adapter-sdk` |
| Referenzierte Binärdaten und begrenzter Abruf unter separatem Grant | Protokoll, SDK, `DataService` und `Host::copy_data` |
| Allgemeine Datenquelle mit persistenten Snapshots und Metadaten-ACK | `adapters/file-data` |
| Zwei verschiedene Nutzer des SDK | Sensor und Lüfter unter `adapters/` |

Das SDK übernimmt die wiederkehrende Protokollarbeit. Ein Treiber ergänzt
sein Manifest und die fachlichen Operationen. Derselbe Adapter kann mehrere
Rollen anbieten, etwa Beobachtungen, gezielte Abfragen und Actions.
Eine Fähigkeit benötigt deshalb keinen eigenen Prozess pro Rolle.

Geräteabhängige Übersetzung, Persistenz und Idempotenz bleiben explizite
Treiberaufgaben. Grants und Freigaben kommen vom Host/Core. Das SDK
erzeugt keine Autorität und prüft keine Suchstrategie.

## Weitere Reihenfolge

1. Die gemeinsame Basis mit den bestehenden Sensor- und Lüfterfällen
   nachweisen: Observe mit ACK und Replay, Request und freigegebene Act-Aufträge,
   korrelierte Fehler sowie abgewiesene falsche Capabilities.
2. Weitere allgemeine Rollen nach Bedarf durchgängig ergänzen. Input,
   Transform, Reason und Output sind im Manifestvertrag vorhanden;
   ihre Unterstützung im stdio-Transport muss gesondert umgesetzt werden.
   Der erste SDK-Ausbau unterstützt Observe, Request und Act.
3. Referenzierte Binärdaten sind als allgemeiner Vertrag implementiert.
   Der blockweise Abruf berücksichtigt Frame-Limits und gesonderte
   Datenlimits des Grants. Die Dateiquelle und das Exportbeispiel weisen
   den vollständigen Prozessweg nach. Eine spätere Kameraquelle kann
   denselben Vertrag verwenden; Bildanalyse und Videostreaming fehlen noch.
4. Die vorhandenen Verträge an einer Simulation erproben. Der spätere
   Drohnenanschluss liefert Zustandsdaten, Kameradaten und Bewegungsfähigkeiten
   über diese allgemeinen Mechanismen. Fachliche Schemas bleiben bei den
   Komponenten.

Die Reihenfolge weiterer Rollen richtet sich nach einem konkreten Bedarf;
der Beginn der Drohnenerprobung erfordert nicht die Implementierung jeder
denkbaren Adapterrolle. Die bereits benötigten Rollen müssen allgemein
nutzbar und unabhängig nachgewiesen sein.

## Abnahme der gemeinsamen Basis

- SDK und Protokoll besitzen keine Core-, Sensor-, Lüfter- oder
  Drohnenabhängigkeit.
- Die gleichen Session- und Routingmechanismen bedienen Sensor und Lüfter.
- Ein neuer fachlicher Adapter implementiert Treiberoperationen; er benötigt
  keine Kopie der Protokollschleife und keine Gerätefallunterscheidung im Core.
- Bestehende Grants, Approval-Grenzen und die Wiederholungssemantik bleiben
  wirksam. Die vorhandenen Prozess- und Neustarttests müssen weiterhin bestehen.

Details zur Implementierung stehen in der
[SDK-Dokumentation](../crates/arcs-adapter-sdk/README.md), der langfristige
Anwendungsnachweis im [Plan für die Drohnensuche](DRONE_SEARCH.md).
