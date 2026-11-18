# ARCS – Zwischenbericht zum begrenzten Fallback, 18. November 2026

Dieser Bericht ergänzt den [Stand bis Slice 14](ZWISCHENBERICHT_2026-11-18.md),
die [Reasoner-Auswahl](ZWISCHENBERICHT_2026-11-18_ROUTING.md), die
[Textsuche](ZWISCHENBERICHT_2026-11-18_MEMORY.md), die
[Aufgabenpausen](ZWISCHENBERICHT_2026-11-18_PAUSES.md) und die
[Reasoning-Belege](ZWISCHENBERICHT_2026-11-18_ATTEMPTS.md).
Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md).
Slice 19 setzt weitere Teile von v3 §9.2/§9.3, §7.2 PAR-05/PAR-07/PAR-10 und
v2 RSN-06 um; die vollständigen Phasenabnahmen bleiben offen.

## Ergebnis

Eine Reasoning-Route kann nach einem belegten Timeout oder einer belegten
Nichterreichbarkeit zu geordneten, ausdrücklich erlaubten Alternativen wechseln.
Die optionale Policy `reasoning_recovery` begrenzt die Kette auf den ersten
Request und höchstens sieben weitere. Die Definition und jeder Schritt werden
als `arcs.reasoning_recovery.v1` beziehungsweise
`arcs.reasoning_recovery_step.v1` im gemeinsamen Artifact-Netz gespeichert.

Ein Schritt bindet Vorgänger, exakten Fehlerbeleg und dauerhafte Fälligkeit.
Konkurrierende SQLite-Verbindungen einigen sich auf den zuerst gespeicherten
Beleg einschließlich seiner Fälligkeit. Ein neuer Versuch erhält eine eigene
Anfrageidentität. Kontextauswahl, Binärreferenzen, Zielschema, erlaubte
Handlungen und andere Constraints bleiben gebunden. Die bestehende
Reasoner-Auswahl wird als historische Erstbindung erhalten.

Ungültige Antworten, Ablehnungen, `no_solution`, Budget-Antworten,
Autorisierungs- und Eingabedatenfehler erlauben keinen Wechsel. Ein Fehler
ohne passenden Core-Abschlussbeleg erlaubt ebenfalls keinen Fallback. Die
Prüfung benutzt strukturierte Fehlertypen und gespeicherte Versionen. Ein
abgeschlossener Fehlversuch kann nicht unter derselben Schrittidentität
erneut ausgeführt oder durch einen verlorenen Queue-Checkpoint verlängert
werden. Aktuelle Grants und Budgets gelten auch für die Alternative.

Ist eine Session aus einem früheren Aufruf weiterhin quarantänisiert,
prüft der Core eine neue, ausdrücklich gebundene Anfrage und dokumentiert
deren lokale Ablehnung. Dabei werden keine Binärquellen gelesen, keine Worker
belegt und keine Daten an die alte Session gesendet. Anschließend kann die
erlaubte Alternative übernehmen. Das ermöglicht weitere Aufgaben und Goal-
Runden im selben Host. Ein Core-Aufrufversuch belegt dabei keine tatsächliche
Backend-Zustellung oder Inferenz; der Host verbindet den alten Prozess nicht
automatisch neu.

## Architektur und Kontrollgrenzen

Die neue Funktion liegt im bestehenden Reason-Bereich des Core. Definitionen,
Schreibpfade, Belegprüfung und Statusansichten sind nach Zuständigkeit getrennt.
Die stabile V2-Invocation-Identität ist nun ein gemeinsamer Core-Helfer für
normalen Hostbetrieb und Recovery. Der reservierte Publisher wird externen
Adaptern verweigert. Veröffentlichte Invocation-, Auswahl- und V2-Schemas
wurden nicht verändert; neue optionale Routenfelder erhalten alte Identitäten.

Ein zusätzlicher Index im bestehenden Aufgabenjournal hält nur aktuelle
Core-Versionsreferenzen für die Anzeige. Die Entscheidung bleibt im Artifact-
Netz. CLI und Monitor zeigen höchstens 50 zuletzt angelegte Recovery-Aufgaben
mit aktuellem Schritt, Auswahl, Fälligkeit, Versuchsnummer und Invocation-
Status. Private Constraints und Kontextpayloads werden dort nicht ausgegeben.

Joins und geladene Plan-Snapshots prüfen den tatsächlichen Produzenten anhand
seines konkreten Recovery-Requests. Die Goal-Verification verlangt ebenfalls
diesen Nachweis für genau die aktuelle Planungsrunde. Diese Prüfung erfolgt
auch beim Review und unmittelbar vor der Ausführung. Die bloße Zugehörigkeit
eines anderen Reasoners zum Katalog reicht nicht aus.

Goals verwenden die Policy nur für die Planung. Ihr Assessor bleibt unabhängig.
Jede Runde erhält eine eigene Kette und eigene Kandidaten. Verification und
konfigurierte Approval bleiben vor jeder Gerätewirkung wirksam. Eine Pause
übernimmt noch fertige Antworten, wählt oder startet währenddessen aber keine
Alternative. Planabbruch und vollständige Lebensdauerfristen sperren noch nicht
ausgeführte Alternativen. Bestehende exklusive Goal-Reservierungen bleiben
gemäß ihren Kontrollregeln erhalten.

Gesamtbudgets erzeugter Pläne rechnen das höchste zulässige Erstbudget und
die Summe aller Recovery-Budgets pro maximaler Goal-Runde ein. Assessment und
Actions werden weiterhin separat gezählt. Fehlende notwendige Tokenlimits,
zu kleine Binärreceiver-Grants und Überläufe werden abgewiesen. Wartezeit
verbraucht keine Worker; `max_lifetime_ms` begrenzt weiterhin die gesamte
Lebensdauer. Definitionen sind auf 60000 Bytes und Ketten auf acht logische
Requests begrenzt.

## Nachweise

| Prüfung | Ergebnis |
| --- | --- |
| ARCS: vollständiger Rust-Workspace einschließlich Doctests | 554 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Binär- und Beispielbuild, Formatierung und `git diff --check` | erfolgreich |
| Clippy, gesamter Workspace und alle Targets | erfolgreich; zwei vorhandene Hinweise in `reasoning/service.rs` |

Der vollständige Rust-Lauf verwendet `cargo test --workspace --locked --offline
--no-fail-fast -- --test-threads=4` in einem separaten Target-Verzeichnis.
Die getesteten Runtimes führen ihre Worker weiterhin parallel aus. Nach der
Korrektur des synchronen Zeitbezugs wurde der gesamte Lauf erneut ausgeführt.
Die Python-Prüfung verwendet `python3 -m unittest discover -s
apps/control-panel/tests -v` nach Ende des Rust-Gesamtlaufs.
Die getrennten Adapter wurden mit ihren vorhandenen Lockfiles offline geprüft.
Ihre Quellen und ihr bisheriger lokaler Veröffentlichungsstatus wurden in
diesem Slice nicht verändert.

Sechs neue Core- und dreizehn neue Prozess-Tests ergänzen die bisherige
Regression.

Die Tests decken tatsächliche Adapterwechsel, unveränderte Erstbindungen,
isolierte unabhängige Arbeit, unzulässige Antworten, manipulierte Requests,
alternative Budgets, begrenzte Ketten, konkurrierende Schreiber, unbekannte
Ausgänge, Neustart und verlorene Fehler-Checkpoints ab. Ein injizierter Fehler
zwischen Core-Abschluss und Recovery-Publikation hinterlässt keinen halben
Schritt; die spätere Wiederaufnahme ruft das fehlgeschlagene Backend nicht
erneut auf. Weitere Tests prüfen pausierte Worker, Fristen, Planabbruch,
erzeugte Schritte, Joins und einen ausgefallenen Root-Planer.
Ein eigener synchroner Prozesstest prüft, dass die Verzögerung erst nach dem
fehlgeschlagenen Aufruf beginnt. Die Host-Runtime behandelt dabei
quarantänisierte Reasoner konsistent in synchronem und parallelem Betrieb.

Der Goal-Prozesstest erreicht nach dem Ausfall des ursprünglichen Planers
zwei aufeinanderfolgende Runden mit dem Ersatzplaner. Beide Gerätewirkungen
verlangen eigene manuelle Freigaben und werden anhand neuer Beobachtungen
bewertet. Ein Kandidat aus einer anderen Runde wird abgewiesen. Nach Neustart
bleiben genau zwei bestätigte Wirkungen erhalten; der ausgefallene Backend-
Prozess hat nur die erste Anfrage tatsächlich erhalten.

Das [startbare Beispiel](../apps/adapter-host/examples/reasoning-recovery.md)
wurde in einem isolierten Verzeichnis mit den vorhandenen externen Solver-
und Modell-Fixture-Prozessen ausgeführt. Eine exklusive Sperre auf der
Beispiel-Solverdatenbank löst einen echten Timeout aus. Die konfigurierte
Alternative übernimmt; ein unabhängiger Auftrag läuft parallel weiter. Die
Historie enthält einen Timeout und zwei gültige Modell-Antworten. Ein
anschließender normaler CLI-Neustart übernimmt genau dieselben drei
Versuchsbelege und denselben Recovery-Schritt.
Auch drei echte Monitor-Snapshots zeigen dieselbe Recovery-Auswahl und die
beiden unveränderten Review-Ergebnisse.

## Grenzen und nächste Arbeiten

Die Policy ist eine begrenzte Folge von Reasoning-Alternativen. Automatische
Wiederverbindung, Auswahl anhand verifizierter Qualität oder Kosten,
Modellvergleich, semantischer Fallback und allgemeine versionierte Neuplanung
bleiben weitere Arbeit. Act-, Request- und Output-Operationen erhalten dadurch
keine zusätzliche Wiederholung oder Autorität.

Unbekannte alte Ausgänge erhalten keinen erfundenen Abschlussbeleg. Ihre
Wiederaufnahme verwendet weiterhin dieselbe Anfrageidentität. Die Zahl
tatsächlicher Backend-Berechnungen bei verlorenen Antworten hängt von der
Deduplizierung des Adapters ab. Die neuen Limits zählen logische Requests mit
bekannten Abschlüssen, einschließlich lokaler Ablehnungen. Scheduler-Zeiten
kommen aus der vertrauenswürdigen Host-Anwendung; ein zurückspringender
Zeitwert kann die gespeicherte Wartezeit verlängern.

Die Beispiele verwenden Solver, Modell-Fixtures und simulierte Geräte. Sie
weisen weder echte Modellinferenz noch reale Drohnenmissionen oder realen
mehrmonatigen Dauerbetrieb nach. Das langfristige universelle Ziel bleibt
unverändert offen.
