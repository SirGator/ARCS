# ARCS – Formale Nachweise mit Lean, 28. November 2026

Dieser Bericht dokumentiert den fünften umgesetzten Slice der Erweiterung
[Zielbilderweiterung v3.1](ZIELBILD_ERWEITERUNG_2026-11-27.md) aus dem Bereich
Formale Methoden (`FORM-01`, `FORM-02`, `FORM-05`, Grundlage von `FORM-03`
und `FORM-04`).

Lean wurde **nicht** zum siebten Manager und **nicht** zu einer zusätzlichen
obersten Kontrollinstanz. Der Core hat die formalen Prüfverträge und die
Verbindlichkeit erweitert; die Lean-Engine läuft als externer Adapter in der
bestehenden Capability-Infrastruktur.

## Ausgangslage

Die Analyse zeigte, dass ARCS Ergebnisse prüfte, aber keine formalen Aussagen
kannte. Das Artefaktmodell war für neue Typen offen, es existierte jedoch
kein Vertrag, der einen Beweis an eine konkrete Aussage, eine Planrevision
und ein Ziel band. Eine vorgeschriebene formale Prüfung war nicht
ausdrückbar: Das Ziel konnte keine Bedingung benennen, vor deren Erfüllung
nicht gehandelt werden durfte.

## Was geändert wurde

### Core: formale Artefakte und ihre Bewertung

- Neu `crates/arcs-core/src/decision/formal/`: `FormalEvidenceService` führt
  Definitionen, Annahmen, Modelle, Beweisobligationen, Beweise, Theorien,
  Gegenbeispiele und Verifikationsberichte als versionierte Artefakte im
  bestehenden Netz. Zehn neue Schemas in `crates/arcs-core/schemas/v1/`
  (`formal-definition`, `formal-assumption`, `formal-model`,
  `proof-obligation`, `proof`, `theorem`, `counterexample`,
  `formal-verification-report`, `formal-check-request`,
  `formal-check-outcome`) sind in die Schema-Registry eingebettet.
- Die Typen des Anhangs 17 (FORM-01) sind damit umgesetzt: `formal_definition`,
  `formal_assumption`, `formal_model`, `proof_obligation`, `proof`,
  `theorem`, `counterexample` und `formal_verification_report`.
- Ein Nachweis ist nur zusammen mit seiner Bindung gültig. `EvidenceBinding`
  verlangt, dass jedes von der Obligation erklärte Feld – Zielversion,
  Planrevision, Ziel – vom Aufrufer bestätigt wird. Was die Obligation nicht
  erklärt, wird nicht erzwungen; ein fehlendes Feld ist kein freier Pass für
  einen beliebigen Kontext.
- `FormalEvidenceService::decide` unterscheidet drei Stufen: `Allowed`
  (Nachweis trägt die Handlung), `Blocked` (Nachweis fehlt, ist ungültig oder
  widerlegt) und `Waiting` (Werkzeug nicht verfügbar). Nur `Waiting` ist
  vorübergehend; ein fehlgeschlagener Nachweis ist endgültig.
- Ein Beweis darf weder mehr Annahmen noch mehr Axiome verwenden, als die
  Obligation erlaubt. `record_proof` lehnt das ab. `sorry`, nicht erlaubte
  Axiome und ein Überschreiten der Laufzeitgrenze führen zu
  `FormalVerdict::Rejected`.
- Neu `prepare_check`: Der Core löst die Obligation zu einer Prüfanfrage auf
  (`arcs.formal_check_request.v1`). Eine inhaltsgleiche Anfrage wird
  wiederverwendet, damit die Prüfung idempotent bleibt und ein Neustart
  keinen zweiten Lauf auslöst.
- Neu `record_outcome`: Der Core nimmt das Ergebnis eines externen Werkzeugs
  entgegen und **bewertet** es. Das Werkzeug wird nicht geglaubt. Aus einem
  widerlegten Ergebnis mit Zeuge entsteht ein Gegenbeispiel-Artefakt.

### Adapter und isolierte Lean-Engine

- Neu `crates/arcs-lean-engine/`: Die Engine startet Lean als Subprozess in
  einem eigenen temporären Arbeitsverzeichnis, mit harter Laufzeitgrenze und
  ohne Netz- oder Dateizugriff jenseits dieses Verzeichnisses. Sie erkennt
  `sorry` und `sorryAx`, extrahiert die verwendeten Axiome über
  `#print axioms` und sucht mit einer Taktikfolge nach einem Beweis. Findet
  sie keinen, versucht sie die Widerlegung der Aussage: ein Beweis der
  Negation belegt, dass die Aussage falsch ist, statt nur, dass kein Beweis
  gefunden wurde.
- Neu `adapters/formal-check/`: Die Engine erscheint als Adapter mit der
  Capability `formal.check` im Vertrag `Transform`. Sie erbt weder Autorität
  noch Store-Zugriff. Das Manifest nennt in seiner Beschreibung die Grundlage
  der Werkzeugkette, damit der Betreiber sie in einer Obligation erlauben
  kann. Der Pfad der Lean-Werkzeugkette ist über `--lean-binary` wählbar.

### Host: Nachweis als Bedingung der Handlung

- Neu `apps/adapter-host/src/task_runtime/formal.rs`: Die Runtime holt den
  Nachweis über die Adapter-Infrastruktur und bewertet ihn gegen den aktuellen
  Zielbezug. `GoalConfig.formal_evidence` benennt die Obligation, die
  Fähigkeit und die Laufzeitgrenze.
- Das Gate entscheidet **vor** dem übrigen Handlungsvertrag. Ohne Nachweis
  wirkt die Handlung nicht. Fehlt der Nachweis, geht das Ziel in einen
  begrenzten Wartezustand (`Verifying`); ist er dauerhaft unbrauchbar, wird
  das Ziel blockiert.
- Ein nicht verfügbares Werkzeug ist ausdrücklich **kein** Fehler der
  Handlung. `request_formal_check` unterscheidet „Werkzeug fehlt" (Timeout,
  nicht verfügbar) von einer verweigerten Prüfung. Der erste Fall führt in
  den Wartezustand, der zweite in einen Fehler.
- Die Operator-Befehle `Obligate` und `Prove` legen eine Obligation an und
  stoßen eine Prüfung an. Sie kommen aus der vertrauenswürdigen Anwendung;
  ein Reasoner kann sie nicht auslösen.

## Nachweise

- `crates/arcs-core/src/decision/formal/tests.rs`: 17 Tests. Anlegen der
  Artefakttypen, strikte Bindung an Ziel und Planrevision, Ablehnung eines
  Beweises mit mehr Annahmen oder mehr Axiomen als erlaubt, unabhängige
  Prüfung eines externen Ergebnisses, Ablehnung eines externen Ergebnisses
  ohne Beweis, Umwandlung eines Widerlegungsbefunds in einen
  `Blocked`-Entscheid, `Waiting` bei nicht verfügbarem Werkzeug.
- `crates/arcs-lean-engine/src/lib.rs`: 6 Tests gegen eine echte
  Lean-Werkzeugkette. Belegbarer Satz wird bewiesen; falscher Satz wird
  widerlegt; `sorry` wird als Befund gemeldet; die Engine sucht einen Beweis
  selbst; fehlende Werkzeugkette und Überschreiten der Laufzeitgrenze ergeben
  `Unavailable`.
- `apps/adapter-host/tests/formal_evidence.rs`: 5 Ende-zu-Ende-Tests mit
  echtem Zielablauf, echtem Lean und echtem Lüfter-Adapter. Ein Ziel wirkt
  erst, nachdem die vorgeschriebene Obligation bewiesen ist; ein fehlendes
  Lean hält das Ziel im Wartezustand, ohne zu handeln; eine widerlegte
  Aussage blockiert das Ziel; eine Obligation eines anderen Ziels trägt
  dieses Ziel nicht; ein bewiesener Nachweis übersteht einen Neustart, ohne
  erneut geprüft zu werden.

Der vollständige Workspace-Testlauf und `cargo clippy --workspace --all-targets`
sind fehlerfrei; die verbleibenden Clippy-Hinweise bestanden bereits vorher.

## Erkenntnisse

- Der Core darf die Grundlage der Werkzeugkette nicht kennen. Er weiß nicht,
  dass Lean auf `propext` ruht. Deshalb nennt die Obligation die erlaubten
  Axiome ausdrücklich, und das Manifest der Fähigkeit veröffentlicht sie. Die
  Engine erweitert die Erlaubnis ihrerseits nicht: Was die Anfrage nicht
  erlaubt, darf der Beweis nicht verwenden. Der erste Ende-zu-Ende-Lauf zeigte
  genau das: Der Beweis wurde zunächst abgewiesen, weil die Engine ihre eigene
  Grundlage stillschweigend hinzugefügt hatte.
- Nicht bewiesen und widerlegt sind verschiedene Aussagen. Der Bericht
  unterscheidet sie, und die Engine versucht die Widerlegung erst, wenn kein
  Beweis gefunden wurde. Das kostet einen Lauf, liefert aber statt eines
  Schweigens einen belastbaren Gegenbeleg.
- Ein fehlendes Werkzeug ist ein Wartezustand, ein falscher Nachweis ein
  dauerhafter Befund. Beide dürfen nicht denselben Fehlerweg nehmen. Die
  Trennung geschieht an der Aufrufstelle, weil nur dort bekannt ist, ob die
  Prüfung nachholbar ist.
- Der Prüfauftrag ist ein Artefakt. Dadurch ist die Prüfung idempotent, sie
  überlebt einen Neustart und ihr Ergebnis bleibt am Auftrag gebunden.
- Die Kopplung an eine Planrevision ist strenger als die an ein Ziel. Für
  plangebundene Ziele ist die Obligation ohne Planrevision zu führen; eine
  Planrevision ohne passenden Nachweis trägt nichts.

## Offen

- `FORM-03` ist nur in der Grundlage umgesetzt: Beweisobligationen und
  Gegenbeispiele sind Artefakte, fließen aber noch nicht als strukturierte
  Rückmeldung in Reasoning und Planung zurück.
- `FORM-04` ist offen: Das Sicherheitsmodell des Core ist noch nicht formal
  gespiegelt. Authority-Erhaltung, Klassifikationsvererbung und sichere
  Zustandsübergänge sind implementiert, aber nicht formalisiert.
- `FORM-06` und `FORM-07` sind offen: Es gibt keine Kombination mit
  SMT-Solvern, Optimierern oder Simulationen, und keine Werkzeuge zur
  Übertragung von Rust-Teilmengen in formale Modelle.
- Die formale Prüfung läuft als Transform im Adapter-Worker. Sie ist durch
  `max_concurrent_transforms` begrenzt, aber sie hat noch keinen eigenen
  Budget- oder Latenzzähler wie das Reasoning (`CTX-07`).
