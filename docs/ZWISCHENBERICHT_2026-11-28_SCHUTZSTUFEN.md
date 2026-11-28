# ARCS – Schutzstufen, erzwungene Ableitung und Default-Deny, 28. November 2026

Dieser Bericht dokumentiert den ersten umgesetzten Slice der Erweiterung
[Zielbilderweiterung v3.1](ZIELBILD_ERWEITERUNG_2026-11-27.md) aus dem Bereich
Datenschutz und kontrollierte Informationsflüsse (`PRIV-01`, `PRIV-02`,
`PRIV-03`, Grundlage von `PRIV-07`).

Umgesetzt wurde kein zweites System und kein zusätzlicher Manager. Die
bestehenden Verträge von Artifact-Factory, Store, Memory-Scope, Adapter-Grant
und Reasoning-Kontext wurden erweitert.

## Was geändert wurde

- Neu `crates/arcs-core/src/core/security.rs`: Schutzstufen `Public`,
  `Internal`, `Confidential`, `Restricted`, `Secret` als geordnete Standardstufe
  S0–S4, das Sicherheitsprofil eines Artefakts mit Deklaration, wirksamer
  Stufe, Eigentümer, Zweck und Herkunftsbeleg sowie die Auflösungsfunktion
  `resolve_classification`.
- `Artifact` trägt ein `security`-Profil. Es wird als JSON-Umschlag gespeichert;
  Altbestände bleiben ohne Migration lesbar (`serde(default)`).
- Neu `crates/arcs-core/src/store/database/sqlite_database/classification.rs`:
  der Store löst die wirksame Schutzstufe beim Commit auf. Quellen sind die
  Provenienz-Eltern, die Vorgängerversion desselben Artefakts und eine vom
  Erzeuger mitgeführte Stufe.
- `append_validated` gibt die aufgelöste Fassung zurück. Gespeichert wird nur
  dann eine Stufe, wenn sie von der Beanspruchung des Erzeugers abweicht.
  Damit bleibt ein erzeugtes Artefakt bytegleich, solange die Herkunft nichts
  Strengeres verlangt.
- `StoreError` kennt `UnauthorizedDowngrade` und `IncompleteDeclassification`.
  Ein abgelehnter Commit hinterlässt keine Spur; der Store bleibt benutzbar.
- `ArtifactFilter` und damit der gesamte Abruf kennen eine Obergrenze
  `max_classification`. Der Abruf filtert vor der Offenlegung, nicht danach.
- `AdapterGrant` erhält `data_classification`. Das Feld wirkt in beide
  Richtungen: Daten des Adapters tragen diese Stufe, und Daten, die den Adapter
  erreichen, dürfen sie nicht überschreiten. Ohne Angabe gilt `internal`.
- Die Kontextauswahl (`ReasoningContextService`) führt die Grenze des
  Betreibers als harte Obergrenze. Ein Scope darf enger sein, niemals weiter.
  Pflichtkontext wird vor der Projektion geprüft.
- Der Reasoning-Ertrag erbt die Stufe des Auftrags und damit die Stufe des
  Kontexts, der an das externe Modell ging.

## Nachweise

- `crates/arcs-core/src/core/security.rs`: 14 Unit-Tests zur Stufenordnung,
  Vererbung, Ablehnung und Belegkonsistenz.
- `crates/arcs-core/src/store/database/sqlite_database/classification_tests.rs`:
  8 Tests. Speichern der Deklaration, Vererbung aus der Herkunft, Ablehnung
  einer unbelegten Herabstufung, zugelassene Declassification, Ablehnung einer
  Declassification durch ein Modell, Schutz vor absenkenden Revisionen,
  Verhalten bei fehlender Quelle, Lesbarkeit von Altbeständen.
- `crates/arcs-core/src/reasoning/context/tests.rs`: 4 Tests. Vertrauliche
  Erinnerung bleibt außerhalb eines engeren Scopes, erreicht aber einen
  ausdrücklich berechtigten Adapter; ein Scope oberhalb der Grant-Grenze und
  ein Pflichtkontext oberhalb der Grenze werden abgewiesen.
- `apps/adapter-host/tests/data_classification.rs`: 2 Ende-zu-Ende-Tests über
  Adaptereingang, Memory und Reasoning-Kontext.

Der vollständige Workspace-Testlauf und `cargo clippy --workspace --all-targets`
sind fehlerfrei; die verbleibenden Clippy-Hinweise bestanden bereits vorher.

## Erkenntnisse

- Die Auflösung gehört in den Store und nicht in die Artifact-Factory. Nur der
  Store kennt die Vorgängerversion und die tatsächlich vorhandenen Quellen.
- Deklaration und wirksame Stufe müssen getrennt gespeichert werden. Andernfalls
  kann ein Erzeuger eine bereits aufgelöste Stufe als eigene Deklaration
  weiterreichen und damit eine Herabstufung vortäuschen.
- Die Grenze muss an der Offenlegungskante liegen, nicht in der Erzeugung. Ein
  Filter im Abruf wirkt erst, wenn er auch den Pflichtkontext erfasst.
- Eine fehlende Quelle senkt die Stufe nicht ab. Sie belegt nur nichts, womit
  sich eine Herabstufung rechtfertigen ließe.

## Offen

- `PRIV-04` Secret-Broker, `PRIV-05` Egress-Isolation, `PRIV-06` Löschung und
  Aufbewahrung sind nicht umgesetzt.
- `PRIV-01` ist nur zur Hälfte erfüllt: Eigentümer und Zweck werden geführt,
  Empfänger, Aufbewahrung und Verarbeitungsumgebung fehlen.
- `PRIV-03` gilt für Artefakte und Graphkanten; Embeddings, Caches und
  Logs sind nicht erfasst.
- `PRIV-07` prüft bisher nur die Datenklasse. Zweck, Betreiber und
  Vertragsvorgaben sind noch nicht Teil der Routingentscheidung.
- Die Zweckbindung (`purpose`) wird gespeichert, aber noch nicht erzwungen.
