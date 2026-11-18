# ARCS – Zwischenbericht zu Aufgabenpausen, 18. November 2026

Dieser Bericht ergänzt den [bisherigen Stand](ZWISCHENBERICHT_2026-11-18.md),
die [Reasoner-Auswahl](ZWISCHENBERICHT_2026-11-18_ROUTING.md) und die
[Textsuche im Gedächtnis](ZWISCHENBERICHT_2026-11-18_MEMORY.md).
Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md).
Slice 17 setzt weitere Teile von v3 §8.2 LNG-05 sowie §7.2
PAR-05/PAR-07/PAR-10 um; die allgemeinen Phasenabnahmen bleiben offen.

## Ergebnis

Aufgaben und ganze Pläne lassen sich nun mit dauerhaften, revisionierten
Betreiberkommandos pausieren und fortsetzen. Die Pause überlagert den bisherigen
fachlichen Zustand, ohne Artefakte, Identität oder bestätigte Ergebnisse zu
verändern. Der Scheduler sperrt auch erzeugte Kindaufträge und Joins. Eigene
Kindpausen bleiben unabhängig von der Elternpause.

Bereits laufende Reasoning-Aufrufe dürfen noch antworten. Der Host übernimmt
und validiert solche Antworten über denselben Core-Pfad wie normale Aufrufe,
startet dabei aber keine neuen Berechnungen. Nach dem tatsächlichen Ende werden
Workerplätze, Adapter-Sessions und benannte Kapazitäten frei. Die Aufgabe
übernimmt ihr fertiges Ergebnis erst nach dem Fortsetzen aus dem Core-Cache.
Wenn ein Fehler bereits im Core gespeichert wurde und sein Queue-Checkpoint
verloren ging, verhindert die Pausebindung einen stillen Neuversuch.

Goals beginnen vor einer neuen Geräteaktion mit neuer Vorbereitung: neue Daten,
neue Anfrage-IDs, neue Verification und Freigabe gemäß Betreiberpolicy. Der alte
Versuch zählt weiterhin gegen `max_rounds`; Pausen erweitern weder Versuchslimits
noch reservierte Gesamtbudgets eines Plans. Wurde noch kein Goal-Checkpoint
erzeugt, setzt die Fortsetzung nur eine neue Aufnahmeschranke und verbraucht
keinen zusätzlichen Versuch.

Nach einem Dispatch werden dessen Wirkung und Bewertung mit den bestehenden
Identitäten abgeschlossen. Eine anschließende Planungsrunde erfasst vor einer
neuen Aktion frische Daten. Ein Checkpoint mit ungeklärtem Dispatch verlangt
beim Fortsetzen ausdrücklich `recover_dispatched: true`; aktuelle Rechte,
Verification und Approval gelten weiterhin. Das ist Recovery desselben
Auftrags, keine pauschale neue Handlungsbefugnis.

Fristen laufen während der Pause weiter. Auch eine von der begrenzten
Ablaufvorprüfung noch nicht erreichte Aufgabe wird vor der Übernahme ihrer
pausierten Worker-Antwort geprüft. Abbruch und Ablauf verwenden die bestehenden
dauerhaften Abbruchpfade. Input und Observe können unabhängig weiterarbeiten.

## Konsistenz und Architektur

Der direkte Runtime-Einstieg und die neue CLI-Option `--task-command` verwenden
denselben Vertrag. Eine Änderung verlangt die erwartete Kontrollrevision und
eine begrenzte Command-ID. Der vollständige ursprüngliche Befehl wird an diese
ID gebunden. Identische Wiederholung liefert denselben Beleg; geänderte oder
veraltete Befehle werden abgewiesen. Eine verspätete Wiederholung einer früheren
Pause kann eine bereits erfolgte Fortsetzung nicht rückgängig machen.

Kontrollbeleg, aktueller Kontrollzustand und betroffene Goal-Checkpoints committen
atomar im bestehenden Aufgabenjournal. Erst danach werden überholte Worker
verworfen oder abschließbare Kontrollsitzungen bereinigt. Ein fehlgeschlagener
Commit kann dadurch keine noch gültige laufende Arbeit verwerfen. Das sind
operative Metadaten derselben Runtime, keine zweite Agentensteuerung oder
separate Gedächtnis-Wahrheitsquelle.

Die normale Antwortübernahme des Reason Managers wurde als gemeinsamer Pfad
herausgelöst. Pause und normale Ausführung verwenden dieselbe Prüfung von
Deadline, Adapterzustand, Grants, Antwort und Core-Persistenz. Die gemeinsamen
Fixtures für erzeugte Goal-Pläne liegen jetzt im vorhandenen Test-Support.

Bei der Prüfung fiel ein wichtiger Rundenübergang auf: Ein Goal kann bereits
den nächsten Planungsschritt beginnen und noch die Dispatch-Referenzen der
vorherigen Runde tragen. Nur `executing`, `observing` und `assessing` behandeln
diese Referenz beim Fortsetzen als laufende Wirkung. Eine neue Planung wird
als Vorbereitung eines eigenen Versuchs behandelt. Ein zusätzlicher Prozess-
Test verhindert, dass derselbe Request mit veränderten Eingaben wiederverwendet
oder ein Budget unbemerkt erweitert wird.

Runtime-Berichte und Monitor-Snapshots zeigen `paused_jobs` und eine begrenzte
Liste `task_controls`, einschließlich Revision, eigener und geerbter Pause und
Hinweis auf nötige Dispatch-Recovery. Die Rust-API bietet außerdem Seiten und
unveränderte Kontrollhistorien. Bestehende Runtime-Definitionen und gespeicherte
Goal-Checkpoints bleiben kompatibel; die Journalmigration ist additiv.

## Nachweise

| Prüfung | Ergebnis |
| --- | --- |
| ARCS: vollständiger Rust-Workspace einschließlich Doctests | 520 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Binärbuild, Formatierung und `git diff --check` | erfolgreich |
| Clippy, gesamter Workspace und alle Targets | erfolgreich; zwei vorhandene Hinweise in `reasoning/service.rs` |

Der vollständige Rust-Lauf verwendet `cargo test --workspace --locked --offline
--no-fail-fast -- --test-threads=4` in einem separaten Target-Verzeichnis.
Die getesteten Runtimes starten ihre Worker weiterhin parallel. Nach der
Korrektur des Rundenübergangs wurde der vollständige Lauf erneut ausgeführt.
Die Python-Prüfung verwendet `python3 -m unittest discover -s
apps/control-panel/tests -v`. Kontrollpanel und separate Drohnenprüfung liefen
ohne konkurrierende große Workspace-Prüfung. Die lokalen Anbindungen wurden
mit ihren vorhandenen Lockfiles offline getestet und in diesem Slice nicht
geändert; ihr dokumentierter lokaler Veröffentlichungsstatus bleibt bestehen.

14 neue Prozess-Tests decken Live-Worker und Kapazitäten, pausierte Planer,
überlagerte Eltern-/Kindpausen, stale Revisionen, Idempotenz, Rollback, Fristen,
verlorene Fehler- und Execution-Checkpoints, neue Goal-Freigaben, Versuchslimits,
erzeugte Goal-Schritte, den Rundenübergang nach einer Wirkung, Aufnahme vor dem
ersten Checkpoint sowie CLI und Migration ab.

Das [startbare Beispiel](../apps/adapter-host/examples/task-pauses.md) wurde mit
echten Adapterprozessen in einem isolierten Verzeichnis geprüft: Ein Goal
wartet zunächst auf manuelle Freigabe, bleibt über Pause und Neustarts gesperrt
und legt nach der Fortsetzung neue Beobachtungen und einen neuen Kandidaten in
Runde 2 vor. Die erste Pause und Fortsetzung erzeugen zwei Kontrollereignisse
und keine Geräteaktion. Auch die
Wiederholung des alten Pausekommandos verändert den fortgesetzten Zustand nicht.
Eine zusätzliche Pause wurde über drei echte Monitor-Snapshots hinweg mit
Kontrollrevision 3, `paused_by: [1]`, unveränderter Runde 2 und weiterhin ohne
Geräteaktion geprüft.
Die Prozess-Tests weisen anschließend echte Freigaben und bestätigte Wirkungen
am simulierten Gerät nach, einschließlich Recovery ohne zweite Wirkung.

## Grenzen und nächste Arbeiten

Die Pause ist kooperativ innerhalb einer Runtime. Sie ist kein physischer
Notstopp, keine harte Unterbrechung synchroner Request-/Act-I/O und keine globale
Aufhebung früherer Core-Freigaben. Exklusive aktive Kontrollsitzungen und
ausstehende Worker bleiben bis zu ihrem zulässigen Abschluss reserviert.
Akzeptierte Jobs und Planreservierungen behalten ihre Queuekapazität.

Die CLI bedient ein gestopptes Journal; ein laufender Host behält dessen
exklusive Sperre. Eingebettete Anwendungen können die API zwischen Ticks
aufrufen. Live-IPC, GUI-Bedienelemente für diese Kommandos, Controller-Fencing,
allgemeine versionierte Neuplanung und eine automatische neue Bewertung bereits
abgeschlossener Ziele bleiben weitere Arbeit. Die Anwendung muss Bediener
authentifizieren; ein Actor-Feld in einem JSON-Dokument ersetzt dies nicht.

Die Prüfungen verwenden Rules-Reasoner, Test-Fixtures und simulierte Geräte.
Sie beweisen weder einen autonomen realen Drohnenflug noch monatelangen realen
Dauerbetrieb. Das langfristige universelle Ziel bleibt unverändert offen.
