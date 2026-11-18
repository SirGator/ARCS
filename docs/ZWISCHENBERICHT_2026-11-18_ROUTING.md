# ARCS – Zwischenbericht zur Reasoner-Auswahl, 18. November 2026

Diese Ergänzung zum [bisherigen Bericht](ZWISCHENBERICHT_2026-11-18.md)
dokumentiert Slice 15. Grundlage bleibt die unveränderte
[ARCHITECTURE.md](ARCHITECTURE.md), insbesondere v3 §9.2/§9.3.

## Ergebnis

ARCS kann für eine Aufgabe tatsächlich zwischen mehreren ausdrücklich
erlaubten Reasonern wählen. Jede Option besitzt eigene Methode und Budgets.
Die Auswahl kombiniert begrenzte lexikalische Eignung, Betreiberreihenfolge
und beobachtete Host-Verfügbarkeit. Ein belegter Solver kann damit vor dem
ersten Aufruf einer freigegebenen Alternative weichen. Wenn keine passende
Option frei ist, wartet die Aufgabe ohne belegten Worker oder Auswahlbeleg.

Der Core speichert die Entscheidung vor dem Dispatch als unveränderliches
`arcs.reasoner_selection.v1` im gemeinsamen Artifact-Netz. Definition,
Eingabe, Aufgabenidentität, erlaubte Optionen und Beobachtungen sind gebunden.
Konkurrierende Verbindungen übernehmen atomar denselben ersten Beleg.
Ein Neustart oder ein unterbrochener Queue-Checkpoint erzeugt keine neue
Auswahl und keinen stillen Wechsel zu einem inzwischen freien anderen Modell.

Private, ausdrücklich konfigurierte Eingabefelder können die Suche steuern.
Ihre Werte bleiben im Core und gelangen dadurch weder in externen Kontext
noch in die neue Statuszusammenfassung. Fehlende, leere oder unpassende
Suchdaten führen zu einem isolierten Aufgabenfehler. Die bestehende
Payloadfreigabe steuert den eigentlichen Datenexport weiterhin separat.

Die konkrete Reasoning-Anfrage referenziert die Belegversion. Der Core prüft
gewählte Capability, Methode, Budget, Zielschema und Eingabebezug und ergänzt
Provenance und `based_on`. Der technische Beleg wertet den Trust der
ursprünglichen Eingaben nicht auf und verbraucht keinen externen Kontextplatz.
Der Core-Publisher ist für externe Installationen gesperrt.

Goals wählen erst bei tatsächlichem Planungsbedarf. Initialer Zielcheck,
Wartephasen und Kontrollübernahme erzwingen keine vorzeitige Planerbindung.
Der Assessor bleibt fest; jede Gerätewirkung durchläuft weiterhin Verification
und manuelle Freigabe. Erzeugte Pläne und ihre Joins akzeptieren den tatsächlich
gebundenen Produzenten. Gesamtbudgets decken konservativ die teuersten
erlaubten Laufzeit-/Tokenoptionen und alle maximalen Goal-Runden ab.

CLI und Monitor bieten eine begrenzte Zusammenfassung unter
`reasoner_selections`. Das [startbare Beispiel und seine Verträge](../apps/adapter-host/examples/reasoner-routing.md)
erklären Auswahlregeln, Konfiguration, private Suchfelder und Wiederaufnahme.

## Architekturkorrekturen

Die Auswahl liegt im bestehenden Core/Reason-Bereich; die Runtime bindet nur
Betreiberpolitik und Aufgabenidentität. Es gibt keine neue Agenteninstanz,
Autoritätsdatenbank oder separate Gedächtnisablage. Die bisherige Identität von
Routen ohne Auswahlpolitik bleibt erhalten.

Der Test mit konkurrierenden SQLite-Verbindungen deckte eine Sperrkonkurrenz
zwischen Belegprüfung und Schreiben auf. Die gemeinsame kurze
Speichertransaktion fordert ihre Schreibreservierung jetzt vor der Prüfung an.
So können konkurrierende Leser nicht erst beim späteren Schreibversuch an
einer veralteten Lesetransaktion scheitern. Externe Operationen liegen weiterhin
außerhalb dieser Transaktion.

Cache-Replay berücksichtigt vorhandene Core-Ergebnisse jetzt auch ohne
benannte Ressourcenpools. Ein bereits berechneter, weiterhin gültiger Request
benötigt dadurch keinen freien Adapter für eine erneute Berechnung. Der Core
validiert Requestidentität, Artefakte und aktuelle Rechte vollständig.

## Nachweis und Grenzen

| Prüfung | Ergebnis |
| --- | --- |
| ARCS: vollständiger Rust-Workspace einschließlich Doctests | 490 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Binärbuild, Formatierung und `git diff --check` | erfolgreich |
| Clippy, Workspace und alle Targets | erfolgreich; zwei vorhandene Hinweise in `reasoning/service.rs` |

Der vollständige Rust-Lauf verwendet `cargo test --workspace --locked --offline
--no-fail-fast -- --test-threads=4` mit einem separaten Target-Verzeichnis.
Die Begrenzung betrifft die gleichzeitig ausgeführten Testfälle; die getesteten
Runtimes starten weiterhin parallele Worker und konkurrierende Aufgaben.
Die Python-Prüfung verwendet `python3 -m unittest discover -s
apps/control-panel/tests -v`. Die separaten lokalen Anbindungen wurden mit
ihren vorhandenen Lockfiles offline getestet und in diesem Slice nicht geändert.
Ihre fehlenden Push-Remotes und der lokale Status sind im vorherigen Bericht
dokumentiert; sie sind nicht Teil der Veröffentlichung dieses ARCS-Commits.

Fünf neue Core-Tests prüfen Policies, unveränderliche Auswahl, echte
SQLite-Konkurrenz, Anfrage-/Graphbindung und gefälschte Belege. Zwölf neue
Prozess-Tests prüfen belegte Optionen, fehlende Kapazität, privates Routing,
Fehlerisolierung, Recovery zwischen Core- und Queue-Commit, alternative
Root-/Schritt-/Goal-Planer, Cache-Replay und konservative Planbudgets. Der
Goal-Test verlangt eine manuelle Freigabe und bestätigt die Wirkung am
simulierten Gerät.

Das Beispiel wurde mit echten Adapterprozessen in einem isolierten Verzeichnis
gestartet. Beide Aufgaben erreichen `ready_for_review`; die ausgewählte
Modell-Fixture wird einmal, der bevorzugte Solver der Auswahlroute keinmal
aufgerufen. CLI-Replay und Monitor erhalten denselben Beleg und erzeugen keine
zusätzlichen Aufträge, Artefakte oder Adapteraufrufe. Die normalen
ID-Generator-Epochen erhöhen sich beim Prozessstart weiterhin.

Die Verfügbarkeitsprüfung ist eine Beobachtung und keine Reservierung. Erst
der konkrete Dispatch prüft gemeinsam alle Eingaben, Workerplätze und
Ressourcenanforderungen der beteiligten Adapter. Bei binären Goal-Quellen
werden vorab Receiver-Grants und Objektzahlen geprüft; Referenzen und Bytes
bleiben Aufgabe der tatsächlichen Anfragevalidierung. Nach dem Einfrieren
kann eine Option warten oder scheitern, ohne automatisch ersetzt zu werden.

Die Modell-Fixture führt keine echte Inferenz aus und liefert im Beispiel
keine optimale Lösung. Das Beispiel weist Routing und Wiederaufnahme nach;
eine fachlich erfolgreiche Gerätewirkung wird separat am simulierten Goal
geprüft. Ein autonomer Drohnenflug oder monatelanger realer Dauerbetrieb ist
damit nicht nachgewiesen.

Weiter offen sind gemessene Erfolgsraten und Kosten, semantische Eignung,
allgemeine Werkzeugauswahl, Ergebnisvergleich mehrerer parallel beauftragter
Reasoner, versionierte Neuplanung und die weiteren offenen Architekturphasen.
