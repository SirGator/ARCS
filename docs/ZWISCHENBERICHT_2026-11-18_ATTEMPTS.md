# ARCS – Zwischenbericht zu Reasoning-Belegen, 18. November 2026

Dieser Bericht ergänzt den [Stand bis Slice 14](ZWISCHENBERICHT_2026-11-18.md),
die [Reasoner-Auswahl](ZWISCHENBERICHT_2026-11-18_ROUTING.md), die
[Textsuche](ZWISCHENBERICHT_2026-11-18_MEMORY.md) und die
[Aufgabenpausen](ZWISCHENBERICHT_2026-11-18_PAUSES.md).
Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md).
Slice 18 setzt weitere Teile von v3 §9.2/§9.3 und §6.5 MEM-03 sowie v2 RSN-04
um. Die allgemeinen Phasenabnahmen bleiben offen.

## Ergebnis

Der Core speichert abgeschlossene Reasoning-V2-Versuche als strukturierte,
unveränderliche Artefakte im gemeinsamen Gedächtnis. Der neue Vertrag
`arcs.reasoning_attempt.v1` unterscheidet gültige Antworten mit Kandidaten,
gültige Antworten ohne Lösung und technische oder vertragliche Fehlschläge.
Die Fehlerklasse entsteht aus dem konkreten Core-Fehlertyp, ohne Interpretation
eines Adapterfehlertexts. Die bestehende technische Wiederholbarkeit bleibt
als `retryable` sichtbar.

Jeder Beleg referenziert Request und exakte Dispatch-Zustandsversion,
Capability, Methode und Budget. Antwortbelege verweisen außerdem auf die
geprüfte Ergebnisversion und enthalten Status, Kandidatenzahl und optional
vom Adapter gemeldete Iterationen beziehungsweise Ausgabetokens. Bei einer
später ausdrücklich ausgelösten Wiederholung bleibt der alte Beleg erhalten;
der neue Versuch erhält eine eigene Dispatch-Version. Ein Cache-Replay erzeugt
keine weiteren Versuche oder Messwerte.

Der Core misst `host_completion_latency_ms` vom vorbereiteten Aufruf bis zum
Beginn der Antwortübernahme. Darin können Datenabruf, I/O und verzögerte
Abholung liegen. Diese Beobachtung ist keine reine Modellrechenzeit und kein
Kostenbeleg. Gemeldeter Adapterverbrauch bleibt ausdrücklich als solcher
gekennzeichnet. `candidate_quality_verified: false` verhindert, dass eine
geprüfte Antwortform oder ein Adapterstatus `solved` als geprüfte Zielerreichung
ausgegeben wird. Die vorhandene Vertrauensstufe der Kandidaten bleibt erhalten.

## Konsistenz und Architektur

Ergebnis, Beleg und abschließender Invocation-Zustand werden gemeinsam im
bestehenden SQLite-Store geschrieben. Das gilt auch für Fehler und die
typisierten Relationen. Scheitert der Commit, bleiben keine getrennt
persistierten Teilbelege zurück. Der finale Zustand verweist auf den Beleg;
der Beleg verweist auf die technischen Eltern und die ursprünglichen
Kontextversionen. So sind auch Fehlschläge aus dem vorhandenen Artifact-Netz
erreichbar.

Die Leseprüfung vergleicht einen übergebenen Beleg mit seiner gespeicherten
unveränderlichen Version. Sie prüft Core-Herkunft, Identität, ursprünglichen
Request, konkreten Dispatch, abschließenden Zustand und gegebenenfalls das
Ergebnis. Eine Kopie mit veränderter Fehlerklasse kann deshalb nicht die
Verknüpfungen eines echten Belegs als Nachweis übernehmen. Der reservierte
Publisher `arcs.reasoning.attempt` wird externen Adaptern verweigert.

Die neuen Speicherhilfen sind allgemeine Store-Funktionen: begrenzte
Ereignisbatches mit einem atomaren Current-State-Übergang, indexierte Seiten
nach Schema und Subject sowie ausgewählte Metadatenprojektionen. Die
Reasoning-spezifische Erzeugung, Prüfung und Historienabfrage liegen getrennt
im vorhandenen Reason-Bereich. Es entsteht keine weitere Agentensteuerung
oder Gedächtnisdatenbank. Die bestehenden Invocation- und V2-Schemas wurden
nicht verändert.

## Begrenzte Historie

`Host::reasoning_attempts` und `--reasoning-attempts query.json` lesen dieselben
Belege. Eine Abfrage benennt eine bis 16 verschiedene, aktuell autorisierte
Reason-Capabilities und verarbeitet höchstens 256 Zeilen pro Seite. Alle
Grants werden vor dem Lesen geprüft. Die injektive, längencodierte
Capability-Zuordnung verhindert eine Vermischung der Scopes. Ein zusätzlicher
Index wird erst nach der vorhandenen Subject-Migration angelegt.

Die Abfrage hält eine obere Sequenzgrenze fest. Pro Capability liest sie
höchstens `limit + 1` passende Zeilen und führt diese nach Sequenz zusammen.
Auch übersprungene zu große oder nicht vertrauenswürdige Zeilen bewegen den
Cursor; Zähler machen sie sichtbar. Inkonsistente Core-Belege führen zu einem
expliziten Fehler. `page_counts` beschreibt ausschließlich die gültigen
Belege der aktuellen Seite, keine globale Erfolgsstatistik.

Belege und projizierte Elternmetadaten sind jeweils auf 32 KiB begrenzt.
Große Eingaben, Fingerprints und Kandidatenpayloads müssen dafür nicht
vollständig in Rust materialisiert werden. SQLite liest und verarbeitet die
gespeicherten JSON-Dokumente weiterhin; die Mengenbegrenzung verspricht keine
harte maximale Ausführungsdauer. Kontextpayloads, Kandidateninhalte und rohe
Adapterfehler werden in der Historie nicht ausgegeben. Korrelations-IDs,
Budgets und Capability-Referenzen bleiben für den berechtigten Betreiber
sichtbar.

Die CLI prüft die höchstens 65536 Bytes große Anfrage vor dem Hoststart.
Für die aktuellen Grants öffnet sie die Adapter mit ihrem Metadaten-Handshake.
Sie startet keine TaskRuntime, empfängt keine neuen Eingaben und führt keine
konfigurierten Modell-, Request- oder Action-Aufträge aus.

## Nachweise

| Prüfung | Ergebnis |
| --- | --- |
| ARCS: vollständiger Rust-Workspace einschließlich Doctests | 535 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Binärbuild, Formatierung und `git diff --check` | erfolgreich |
| Clippy, gesamter Workspace und alle Targets | erfolgreich; zwei vorhandene Hinweise in `reasoning/service.rs` |

Der vollständige Rust-Lauf verwendet `cargo test --workspace --locked --offline
--no-fail-fast -- --test-threads=4` in einem separaten Target-Verzeichnis.
Die getesteten Runtimes starten ihre Worker weiterhin parallel. Die
Python-Prüfung verwendet `python3 -m unittest discover -s
apps/control-panel/tests -v` nach Ende des Rust-Gesamtlaufs.
Die getrennten Drohnen- und Modelladapter wurden mit ihren vorhandenen
Lockfiles offline getestet. Ihre Quellen und ihr bisheriger lokaler
Veröffentlichungsstatus wurden in diesem Slice nicht verändert.

Die neuen Tests prüfen gültige Antworten, `no_solution`, technische Ausfälle,
ungültige Antwortpayloads, explizite Wiederholung, unveränderte alte Belege,
gefälschte oder manipulierte Belege, aktuelle Grants, Seitengrenzen, große
Dokumente, atomaren Rollback und Wiederöffnung des Stores. Prozesstests zeigen
auch die Core-Übernahme während einer Aufgabenpause und Recovery nach einem
verlorenen Queue-Checkpoint, jeweils ohne weiteren Backend-Aufruf.

Das [startbare Beispiel](../apps/adapter-host/examples/reasoning-attempts.md)
wurde mit echten Adapterprozessen in einem isolierten Verzeichnis ausgeführt:
Vier Versuche lieferten zwei Antworten mit Kandidaten und zwei Antworten ohne
Lösung. Zwei aufeinanderfolgende Historienabfragen und ein vollständiger
Runtime-Neustart erhielten genau dieselben vier Belegversionen und Seiten.
Die nicht freigegebenen privaten Eingaben erscheinen darin nicht.

## Grenzen und nächste Arbeiten

`coverage: "recorded_completed_v2_attempts"` bezeichnet tatsächlich neu
gespeicherte V2-Abschlüsse. Alte V1-/V2-Aufrufe werden nicht nachträglich als
gemessene Ereignisse ausgegeben. Unbekannte Ausgänge nach Absturz, noch laufende
oder verworfene Worker und veraltete Completions erhalten keinen erfundenen
Ergebnisbeleg. Ein technischer Fehler des Core kann weiterhin verhindern,
dass ein Abschluss überhaupt gespeichert wird.

Automatische Wiederholung, Fallback zu einem anderen Reasoner, allgemeine
versionierte Neuplanung, verifizierte Fähigkeitsqualität und Kostenoptimierung
bleiben weitere Arbeit. Die bestehende dauerhafte Reasoner-Auswahl bleibt
unverändert verbindlich. Die strukturierten Belege schaffen dafür eine
prüfbare Grundlage; sie ersetzen keine Verification, Approval oder Kontrolle
der tatsächlich erreichten Wirkung. Die Beispiele verwenden Solver und
Modell-Fixtures; echte Modellinferenz und reale Drohnenmissionen sind dadurch
nicht nachgewiesen. Das langfristige universelle Ziel bleibt offen.
