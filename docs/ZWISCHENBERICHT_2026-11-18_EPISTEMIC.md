# ARCS – Zwischenbericht zu Gedächtnisbewertungen, 18. November 2026

Dieser Bericht ergänzt den [Wiederverbindungsbericht](ZWISCHENBERICHT_2026-11-18_RECONNECT.md).
Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md).
Slice 21 setzt weitere Teile von v3 §6.4 und §6.5 MEM-03/MEM-04/MEM-05/
MEM-08/MEM-09 sowie v2 MEM-06 um. Die Gesamtabnahme dieser Phasen bleibt offen.

## Ergebnis

Ein authentifizierter Betreiber kann konkrete gespeicherte Artifact-Versionen
als `active`, `disputed`, `retracted` oder `superseded` bewerten. Eine Bewertung
enthält den vollständigen Zustand, Betreiber, Grund, bis zu 16 Belegversionen,
optionale UTC-Zeitgrenzen und bei Ersatz die neue Quellen-Version. Sie wird
als `arcs.memory_assessment.v1` im gemeinsamen Artifact-Netz gespeichert.
Die ursprüngliche Quelle einschließlich Payload und Trust bleibt erhalten.

Änderungen verlangen die erwartete Bewertungs-Version. Zustand, Current-Head
und Provenienzrelationen werden atomar geschrieben. Konkurrierende Änderungen
können sich nicht überschreiben. Eine genaue Wiederholung des jüngsten Befehls
liefert denselben Beleg; ältere Befehle nach einer weiteren Revision schlagen
als Konflikt fehl. Gründe und Belegbezüge bleiben in allen Revisionen erhalten.

Die optionale `memory.epistemic`-Policy begrenzt zusätzlichen Reasoning-Kontext.
Aktive Erinnerungen sind innerhalb ihres Gültigkeitszeitraums zugelassen.
Unbewertete und strittige Informationen benötigen jeweils eine ausdrückliche
Policyentscheidung. Zurückgezogene, ersetzte, noch nicht gültige und abgelaufene
Informationen werden ausgeschlossen. Zeitgrenzen verwenden kanonische
UTC-Sekunden; der Beginn gilt einschließlich, das Ende ausschließlich.

Bewertungen beziehen sich auf genaue Versionen und werden nicht auf spätere
Quellversionen vererbt. Eine Ersatzreferenz importiert keine fremden Daten.
Schema, Subject, Tags, Textfilter, Feldauswahl und Größenbudgets begrenzen den
Abruf weiterhin. Source-Trust und Action-Rechte werden durch eine Bewertung
nicht erhöht. Eine aktive Bewertung ist eine dokumentierte Betreiberentscheidung
und kein automatischer Wahrheitsbeweis.

## Architektur und Umordnung

Die fachliche Zuständigkeit liegt in `crates/arcs-core/src/memory.rs` und dessen
Record-Modul. Der Dienst nutzt dieselbe SchemaRegistry und denselben Store wie
Goals, Reasoning und die übrigen Manager. Er enthält keine weitere
Gedächtnisdatenbank und keinen zusätzlichen Agenten. Der Host stellt lediglich
Betreiberzugang und Fehlerdarstellung bereit.

Der Store stellt mit `schema_subject_head_at` einen allgemeinen historischen
Head-Abruf bereit. Er nutzt den vorhandenen Schema-/Subject-/Sequenzindex,
liest höchstens eine Zeile und begrenzt deren serialisierten Inhalt. Ein zu
großer Beleg wird ausdrücklich zurückgewiesen und nicht als fehlende Bewertung
interpretiert. Fachliche Status- und Zeitlogik liegt außerhalb des SQL-Stores.
Schreibvorgänge verwenden den bereits vorhandenen gemeinsamen CAS-Pfad.

Die Kontextauswahl trennt ihre Bewertungsprüfung in ein eigenes Untermodul.
Bei aktivierter Policy entsteht `arcs.context_selection.v2`, einschließlich
Auswertungszeitpunkt und der Zulassungsentscheidung für jeden begrenzten
Suchtreffer. Die jeweilige Bewertungs-Version wird an der festen Artifact-
Commit-Grenze der Suche aufgelöst. Auch ausgelassene Quellen und ihre
Bewertungen werden durch Provenienz und `based_on` belegt.

Ein Replay prüft dieselben Belege an derselben Commit-Grenze und zum selben
Auswertungszeitpunkt. Neue Requests und neue Goal-Runden sehen spätere
Bewertungen und neue Zeitbedingungen. Eine spätere Rücknahme verändert keine
bereits gespeicherte Invocation. Der stabile Auswahlschlüssel bleibt über die
Schema-Versionen hinweg erhalten, damit ein Policywechsel nicht unter derselben
Request-ID eine zweite Auswahl erzeugen kann. Bestehende V1-Schemas werden
nicht verändert.

`--memory-command` unterstützt `assess` und eine begrenzte `inspect`-Abfrage.
Die CLI validiert Dateien bis 65.536 Bytes vor dem Hoststart und führt keinen
Tasklauf aus. Das Hostkonto ist die lokale Zugriffsgrenze; eingebettete
Anwendungen authentifizieren `decision_by` vor dem Dienstaufruf. Das Auditfeld
selbst stellt keine Anmeldung dar. Externe Installationen dürfen den neuen
Core-Publisher nicht belegen. Bewertungsgründe, Betreiber und zusätzliche
Beleg-Payloads werden nicht an Reasoner exportiert.

## Prüfungen

Sieben neue Core-Tests prüfen Revisionen, genaue Wiederholung, unveränderte
Quellen, Provenienz, Zeitgrenzen, strittige Aussagen, historische Commit-Grenzen,
ungültige Rollen und Referenzen, nicht vertrauenswürdige Belege, begrenzte
Reads, konkurrierende Connections und einen injizierten Fehler beim Schreiben
der Provenienzrelationen. Der Rollback erhält sowohl den alten Head als auch
den vorherigen Artifact-Bestand.

Fünf neue Kontext-Tests prüfen tatsächliche Wire-Projektionen, private Felder,
fremde Projekt-Scopes, Ersatzreferenzen und die getrennten Policyentscheidungen.
Simulierte Monate zeigen: Ein alter Request behält seinen Kontext; ein neuer
Request nach dem Ablaufdatum lässt die Erinnerung aus. Policywechsel unter
derselben Request-ID werden zurückgewiesen.

Fünf Prozess-Tests verwenden echte JSON-Input- und Reasoner-Prozesse. Sie prüfen
die tatsächliche Adaptereingabe, Bewertungen über die CLI, Vorabvalidierung,
reservierte Publisher und Wiederaufnahme. Ein weiterer Auftrag wird über den
vorhandenen `arcs-json-input --enqueue`-Pfad aufgenommen: Nach einem Widerruf
enthält nur der neue Auftrag keinen Erinnerungskontext. Die bereits erledigte
Arbeit wird nicht wiederholt.

Das [startbare Beispiel](../apps/adapter-host/examples/memory-assessments.md)
verwendet die vorhandenen Input- und Solverprozesse mit vier Notizen und einer
Zahlenaufgabe. Der isolierte Lauf und sein Replay ergeben einen Review-Auftrag,
einen unveränderten Reasoning-Versuchsbeleg, eine ausgewählte Notiz und zwei
explizite Ausschlüsse. Die fremde Projektnotiz ist kein Suchtreffer.
Bewertungs- und Auswahl-Versionen bleiben beim Neustart identisch. Die
CLI-Inspektion liefert die zwei gesetzten Bewertungen; auch ein normaler
CLI-Runtime-Neustart erhält den fertigen Review-Auftrag.

Die Prüfung umfasst auch Quellversions-IDs mit den maximal zulässigen 512
Bytes. Die interne Subject-Zuordnung verwendet eine kurze abgeleitete Identität;
beim Lesen wird die vollständige Ziel-ID im Beleg geprüft. So verlängert ein
Präfix nicht versehentlich eine bereits maximal lange Quellen-ID über die
Envelope-Grenze hinaus.

Die Prüfprotokolle und der isolierte Beispieldurchlauf liegen lokal unter
`var/verification-memory/`. Die bestehenden zwei Clippy-Hinweise in
`reasoning/service.rs` bleiben unverändert. `ARCHITECTURE.md`, Quellen der
getrennten Adapterprojekte und deren bisheriger Veröffentlichungsstatus wurden
durch diesen Slice nicht verändert.

| Prüfung | Ergebnis |
| --- | --- |
| Vollständiger Rust-Workspace einschließlich Doctests | 590 bestanden, 0 fehlgeschlagen |
| Core nach der Grenzkorrektur gesondert | 251 bestanden, 0 fehlgeschlagen |
| Neue Prozess-Tests gesondert | 5 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Binär- und Beispielbuild, Formatierung, Diff-Prüfung und neue Dokumentationslinks | erfolgreich |
| Clippy für Workspace und alle Targets | erfolgreich; zwei bestehende Hinweise |

Der vollständige Rust-Lauf verwendet `CARGO_TARGET_DIR=target/verification-memory
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1`.
Das Kontrollpanel wurde mit `python3 -m unittest discover -s
apps/control-panel/tests -v` geprüft. Die getrennten Rust-Projekte wurden mit
ihren unveränderten Lockfiles offline getestet. Die gesonderten Core- und
Prozess-Läufe sind zusätzliche Prüfungen und werden nicht zur Zahl 590 addiert.

Der abschließende Workspace-Lauf startet die Tests mit `--test-threads=1`
nacheinander. Explizite Parallelitätsfälle verwenden weiterhin mehrere
Worker und Connections. Zwei Vorläufe mit vier gleichzeitig gestarteten
Tests meldeten jeweils einen vorhandenen Timingfehler: `DatabaseBusy` bei
der ID-Vergabe beziehungsweise einen Timeout beim Start des Lüfteradapters.
Beide unveränderten Tests bestehen im abschließenden Lauf. Assertions und
Fristen wurden nicht gelockert.

## Grenzen und nächste Arbeiten

Bewertungen werden derzeit ausdrücklich durch einen berechtigten Betreiber
gesetzt. Automatische semantische Widerspruchserkennung, Konsolidierung und
kalibrierte Aussagewahrscheinlichkeiten bleiben offen. Ohne aktivierte
Abrufpolicy bleibt der bisherige Vertrag erhalten. Die begrenzte Kandidatensuche
wird nach Ausschlüssen nicht unbegrenzt fortgesetzt.

Die Bewertungsmetadaten bleiben im Core. Die Wire-Sicht enthält weiterhin
Quellversions-ID, Trust und zugelassene Payloadfelder; auch ausdrücklich zugelassene
strittige Erinnerungen erhalten kein zusätzliches Modell-Wirefeld.

Ein Widerruf pausiert oder beendet keine schon laufende Aufgabe und widerruft
keine bestehende Action-Freigabe. Dafür sind die vorhandenen Steuerbefehle und
die weiterhin getrennten Verification-/Approval-Prüfungen zuständig.
Allgemeine Neuplanung bei geänderten Informationen ist weitere Arbeit.
`task_runtime/planning/engine.rs::advance_plan` beendet einen Plan mit
gescheiterten Voraussetzungen derzeit als blockiert. Neue Planrevisionen
müssen künftig die bisherige Ausführungshistorie, verbrauchte Budgets und
noch ungeklärte externe Wirkungen erhalten.

Der Solver nutzt die Notiz nicht fachlich. Der Nachweis belegt Abrufregeln und
Replay, keine allgemeine Verbesserung von Modellantworten. Reale Modellinferenz,
autonome Drohnensuche, Controller-Fencing und monatelanger realer Betrieb
sind mit diesem Slice nicht nachgewiesen.
