# Zwischenbericht: Abstraktion und lokale Rekonstruktion (28. November 2026)

Dieser Bericht beschreibt den achten Slice der Zielbilderweiterung
([ZIELBILD_ERWEITERUNG_2026-11-27.md](ZIELBILD_ERWEITERUNG_2026-11-27.md)):
eine lokale Abstraktion, ein symbolisches Reasoning über Variablen statt
Werten und die lokale Rekonstruktion der konkreten Lösung. Er schließt
zugleich eine Lücke im Default-Deny der Offenlegungsgrenze.

## Ausgangslage

Nach dem sechsten und siebten Slice waren Schutzstufen, erzwungene
Ableitung, Herkunftsbindung, Secret-Broker und Egress-Grenzen umgesetzt
(`PRIV-01` bis `PRIV-05`). Zwei Lücken blieben:

1. **Ein vertraulicher Sachverhalt war extern nicht bearbeitbar.** Die
   Datenklassenprüfung am Modellrand verweigert jeden Kontext oberhalb der
   Grenze des Adapters. Das ist richtig, aber es bedeutet: Wer eine starke
   externe Fähigkeit nutzen will, kann das nur mit internen Daten.
   `PRIV-08` bis `PRIV-10` verlangen genau den anderen Weg: lokal
   abstrahieren, extern bearbeiten, lokal rekonstruieren.
2. **Die v2-Reasoning-Kante prüfte die Schutzstufe nicht.** Die ältere
   v1-Kante tut das seit dem dritten Slice, die v2-Kante (`general.rs`)
   vertraute dagegen auf die Herkunft des Auftrags. Ein direkt
   konstruierter Auftrag konnte damit vertraulichen Kontext an ein
   externes Modell geben. Das widprach `PRIV-02` („Prüfung vor
   Offenlegung, nicht erst nach dem Modellaufruf").

## Änderungen

### Abstraktion als freigegebene Herabstufung (`PRIV-08`)

* Neues Kernmodul `crates/arcs-core/src/privacy/abstraction.rs`:
  * `AbstractionPolicy` ist die Freigabe des Betreibers. Sie nennt Zweck,
    Zielstufe, Empfänger, die als Variablen erlaubten Felder, die
    wörtlich sichtbaren Felder und die Höchstzahl der Variablen. Jedes
    Feld, das weder abstrahiert noch sichtbar ist, wird nicht gesendet.
    Für die Felder gilt Default-Deny.
  * `abstract_payload` zerlegt die Nutzlast lokal: Jedes skalare Blatt
    eines abstrahierten Feldes wird durch `{"$var": "vN"}` ersetzt, die
    Struktur bleibt erhalten. Die Art einer Variable (`text`,
    `identifier`, `number`, `quantity`, `opaque`) wird vom Betreiber
    festgelegt, nicht aus dem Wert abgeleitet.
  * `verify_no_disclosure` ist der Nachweis der Transformation: Findet
    die Prüfung einen konkreten Wert im abstrakten Problem, etwa weil ein
    sichtbares Feld ihn wiederholt, wird die Abstraktion verworfen.
* `AbstractionService` (`abstraction_service.rs`) speichert zwei
  Artefakte:
  * `arcs.abstract_problem.v1` trägt das abstrakte Problem ohne Werte.
    Die Herabstufung auf die Zielstufe ist eine ausdrücklich
    freigegebene, nachvollziehbare Transformation mit Herkunftsbeleg
    und Begründung; der Store löst sie wie jede andere Herabstufung
    auf.
  * `arcs.abstract_binding.v1` trägt die Zuordnung Platzhalter zu Wert
    und erbt die Schutzstufe der Quelle. Diese Tabelle bleibt im Core.
* Das abstrakte Problem vermerkt das Restrisiko
  `pseudonymization_is_not_anonymization`. Über mehrere Abfragen bleibt
  ein Rückschluss möglich; das steht am Artefakt und damit im Bericht.

### Symbolisches Reasoning über die Abstraktion (`PRIV-09`)

* Der ausgeschriebene Nachrichtenvertrag ist das abstrakte Problem:
  Variablen, Schnittstellen und Regeln statt Werten. Der externe
  Reasoner liefert einen Vorschlag, der Platzhalter benutzt.
* Eine Abstraktion gehört genau einem Empfänger. Das abstrakte Problem
  nennt den freigegebenen Reasoner, und die Offenlegungsgrenze prüft
  das bei jedem Zugriff.

### Lokale Rekonstruktion (`PRIV-10`)

* `reconstruct` setzt die Platzhalter lokal ein: Ein ganzer Platzhalter
  wird durch den gebundenen Wert ersetzt, ein Platzhalter in einem Text
  nur bei textuellen Variablen. Ein unbekannter oder nicht einsetzbarer
  Platzhalter bricht ab; es wird nichts geraten.
* Das rekonstruierte Ergebnis wird gegen den Zielvertrag geprüft und
  als Artefakt gespeichert, das von der Quelle und vom abstrakten
  Problem erbt. Die konkrete Lösung ist damit so sensibel wie ihr
  Anlass.

### Default-Deny an beiden Reasoning-Kanten (`PRIV-02`)

* Die v2-Kante prüft jetzt die Schutzstufe jedes Kontextartefakts gegen
  die Grenze des Grants, bevor der Endpunkt den Auftrag sieht.
* Beide Kanten prüfen zusätzlich, ob ein abstraktes Problem für den
  anfragenden Reasoner freigegeben ist.

## Nachweis

| Anforderung | Nachweis | Test |
| --- | --- | --- |
| Abstraktion trägt keine Werte | Das abstrakte Problem enthält weder `C-4711` noch `12500` | `privacy::abstraction_tests::an_abstract_problem_carries_no_concrete_value` |
| Default-Deny für Felder | Ein nicht freigegebenes Feld erreicht das Problem nicht | `privacy::abstraction_tests::a_field_outside_the_release_never_reaches_the_problem` |
| Transformation wird geprüft | Ein sichtbares Feld, das den sensiblen Wert wiederholt, wird abgewiesen | `privacy::abstraction_tests::a_visible_field_that_repeats_a_sensitive_value_is_refused` |
| Typ der Variable stimmt | Ein Zahlfeld als Textvariablen wird abgewiesen | `privacy::abstraction_tests::a_field_kind_that_does_not_match_the_value_is_refused` |
| Unbekannter Platzhalter | Die Lösung wird abgewiesen, es wird nicht geraten | `privacy::abstraction_tests::a_solution_with_an_unknown_variable_is_refused` |
| Zahl wird nicht in Text eingesetzt | Eine Zahlvariable in einem Text wird abgewiesen | `privacy::abstraction_tests::a_number_variable_is_not_substituted_into_a_text` |
| Rekonstruktion setzt Werte lokal | Textvariable und Ganzwert werden korrekt eingesetzt | `privacy::abstraction_tests::a_text_variable_is_substituted_into_a_text` |
| Herabstufung ist belegt | Das freigegebene Problem trägt die Zielstufe und eine Begründung | `privacy::abstraction_tests::an_abstract_problem_is_released_while_the_source_stays_protected`, `reasoning::abstraction_tests::a_released_problem_stays_classified_at_its_target` |
| Bindung bleibt geschützt | Die Bindungstabelle erbt die Stufe der Quelle | `privacy::abstraction_tests::an_abstract_problem_is_released_while_the_source_stays_protected` |
| Zielstufe über der Grenze | Eine Freigabe über der Grant-Grenze wird abgewiesen | `privacy::abstraction_tests::a_target_above_the_grant_ceiling_is_refused` |
| Ergebnis erbt den Anlass | Das rekonstruierte Ergebnis ist so sensibel wie die Quelle | `privacy::abstraction_tests::a_reconstructed_result_inherits_the_protection_of_its_source` |
| Fehlgeschlagene Rekonstruktion hinterlässt nichts | Die Artefaktzahl bleibt unverändert | `privacy::abstraction_tests::an_incomplete_solution_leaves_no_result` |
| Vertraulicher Kontext wird abgewiesen | Der externe Reasoner wird nicht gerufen | `reasoning::abstraction_tests::a_confidential_context_is_refused_at_the_disclosure_edge` |
| Extern wird nur abstrakt gearbeitet | Der Draht des Reasoners enthält keinen Wert, nur Platzhalter | `reasoning::abstraction_tests::an_abstraction_is_solved_externally_and_reconstructed_locally` |
| Abstraktion gehört einem Reasoner | Ein fremder Reasoner wird abgewiesen und nicht gerufen | `reasoning::abstraction_tests::an_abstraction_is_not_released_to_another_reasoner` |

## Erkenntnisse

1. **Der bestehende Herkunftsmechanismus trägt die Abstraktion bereits.**
   `SecurityProfile::declassified` verlangt Quelle, Begründung und einen
   deterministischen Erzeuger. Die Abstraktion musste deshalb keinen
   eigenen Freigabemechanismus bauen; sie erzeugt eine echte, belegte
   Herabstufung, die der Store wie jede andere auflöst. Das war der
   entscheidende Befund, weil eine Sonderfreigabe neben dem Store eine
   zweite Wahrheit geschaffen hätte.
2. **Zwei Artefakte statt eines.** Problem und Bindung gehören in
   getrennte Schutzklassen: Das Problem muss die Zielstufe tragen, die
   Bindung die Stufe der Quelle. In einem gemeinsamen Artefakt müsste
   die niedrigere Stufe gelten, und die Werte wären lesbar.
3. **Der Empfänger gehört in das Artefakt.** Die Freigabe „an diesen
   Reasoner" ist nur wirksam, wenn sie am Rand geprüft wird. Deshalb
   nennt das abstrakte Problem den Empfänger, und die Reasoning-Kante
   vergleicht ihn mit dem anfragenden Adapter.
4. **Die Prüfung der Transformation ist kein Formalismus.** Der Fall, in
   dem ein sichtbares Feld den sensiblen Wert wiederholt, ist real: Er
   entsteht, wenn der Betreiber ein Feld für nicht sensibel hält, das
   faktisch sensibel ist. Die Prüfung wandelt diesen Irrtum in einen
   Abbruch statt in eine Offenlegung.
5. **Default-Deny gehört an jede Kante, nicht an den Aufrufer.** Die
   v2-Kante prüfte die Schutzstufe nicht, weil sie darauf vertraute,
   dass der Kontext über den Context-Service gebildet wurde. Eine
   Prüfung, die vom Weg des Auftrags abhängt, ist keine Grenze. Die
   Prüfung steht jetzt dort, wo die Offenlegung geschieht.
6. **Kurze Werte sind ein bewusster Kompromiss.** Die Offenlegungsprüfung
   greift erst ab vier Zeichen. Kürzere Werte tragen zuwenig
   Information, um eine Offenlegung zu belegen, und würden zufällige
   Treffer in sichtbaren Feldern auslösen. Für kurze Geheimnisse bleibt
   deshalb der Tresor der richtige Weg.

## Offene Punkte

* Die Abstraktion ist bisher eine Core-Fähigkeit. Sie ist noch nicht an
  den Host angebunden: Es gibt noch keinen Betreiberbefehl, der eine
  Abstraktion vorbereitet und eine Lösung rekonstruiert.
* `PRIV-08` ist in der Grundlage umgesetzt. Offen bleiben die in der
  Erweiterung genannten Messgrößen: Erfolgsqualität, offengelegte
  Information, Reidentifikationsrisiko, Kosten, Latenz und Fehlfreigaben
  gegenüber lokalen und nicht abstrahierten Baselines. Das ist die
  Forschungsfrage des Abschnitts und noch nicht Teil der Abnahme.
* `PRIV-09` ist für die Nutzlast umgesetzt. Generierter Code aus einem
  externen Reasoner bleibt vor der Ausführung zu prüfen und zu
  isolieren; die vorhandene isolierte Ausführung gilt, eine eigene
  Codeprüfung für abstrakte Pläne fehlt.
* `PRIV-10` ist für die Rekonstruktion umgesetzt. Eine formale Prüfung
  der Anwendbarkeit (`FORM-06`) ist offen.
* `PRIV-11` (Confidential Computing, Differential Privacy, MPC, HE) bleibt
  eine Forschungsaufgabe.
* Die Abstraktion schützt nur den Weg über den Core. Ein Operator, der
  die Bindungstabelle selbst liest, umgeht die Schutzstufe nicht, aber
  er trägt die Verantwortung.

## Reproduktion

```bash
cargo test -p arcs-core --lib privacy
cargo test -p arcs-core --lib reasoning::abstraction
```
