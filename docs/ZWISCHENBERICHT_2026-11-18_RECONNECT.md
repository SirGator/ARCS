# ARCS – Zwischenbericht zur Wiederverbindung, 18. November 2026

Dieser Bericht ergänzt die bisherigen Zwischenberichte, zuletzt den
[begrenzten Fallback](ZWISCHENBERICHT_2026-11-18_RECOVERY.md).
Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md).
Slice 20 setzt weitere Teile von v3 §7.2 PAR-02/PAR-04/PAR-08/PAR-09/PAR-10/
PAR-11, §10.1 SEC-05 und v2 OPS-01 um. Die vollständigen Phasenabnahmen
bleiben offen.

## Ergebnis

Ein laufender Host kann ausgefallene reine Reason-Installationen mit einer
ausdrücklichen `reasoning_reconnect`-Policy neu verbinden. Es gibt höchstens
16 solche Installationen und zwei gleichzeitige Handshake-Worker. Die Policy
begrenzt Versuche je Störung, Verzögerung und Metadaten-Timeout. Ein zyklischer
Umlauf berücksichtigt fällige Installationen. Alte Endpoint-Nutzer und belegte
Kapazitäten verbrauchen keinen weiteren Versuch.

Der Core speichert Definition und revisionierte Zustände im vorhandenen
Artifact-Netz. Vor dem Prozessstart wird der Versuch mit erwarteter Revision
beansprucht. Die Zustände `healthy`, `waiting`, `connecting`, `exhausted` und
`rejected` sowie Timeout, Transportfehler, Vertragsabweichung und Abbruch bleiben
nachvollziehbar. Wiederholtes Polling verschiebt eine gespeicherte Fälligkeit
nicht. Ein erfolgreicher Metadaten-Handshake beendet die Störung; eine neue
Störung erhält ihre eigene Generation. Eine neue Hostinstanz kann nur einen
tatsächlich erfolgreichen Startup-Handshake als gesund melden.

Wiederverbindung und Task-Recovery sind getrennt. Eine erlaubte neue
Reasoning-Anfrage kann die reparierte Verbindung nutzen. Bereits gescheiterte
Tasks, verbrauchte Request-Versuche, Kandidaten, Freigaben und bestätigte
Wirkungen werden nicht zurückgesetzt. Neue wartende Arbeit kann während der
begrenzten Verbindungsversuche ohne zusätzlichen Reasoning-Worker warten.

## Architektur und Umordnung

Der stdio-Transport trennt jetzt den reinen Handshake von der Registrierung.
Ein Worker startet ausschließlich den vorbereiteten Prozess und liest dessen
Metadaten. Er besitzt keinen Core-Store und registriert keine Fähigkeiten.
Der Core prüft danach das vollständige Manifest und den Grant gegen die
gespeicherte Installation und aktuelle Registry. Abweichungen werden abgewiesen.
Der normale RPC-Timeout wird nach dem gesondert begrenzten Handshake wieder
hergestellt.

Die Session wird erst nach dem erfolgreichen Core-Commit übernommen. Bei
einem fehlgeschlagenen Commit bleibt das fertige Handshake-Ergebnis im Host
zurückgehalten. Die spätere Wiederholung dieses Commits benötigt keinen neuen
Prozessstart. Neue Connections bleiben durch dieselben benannten Kapazitäten
begrenzt wie Reasoning-Arbeit. Andere Tasks und Sensoren können währenddessen
fortschreiten.

Die bisherigen reservierungsspezifischen SQL-Updates wurden zu einem
allgemeinen optimistischen Current-State-Schreibpfad im Store zusammengeführt.
Jeder Zielzustand braucht einen passenden Guard nach Subject und Schema;
zusätzliche Guards können verbundene Ressourcen schützen. Alle Zustände,
Head-Updates und Provenance-Relationen committen atomar. Die fachliche
Reservierungsprüfung bleibt im Reservierungsdienst; dessen bisherigen
Konkurrenz-, Freigabe- und Rollback-Tests bleiben wirksam.

Initialer Prozessstart und Wiederverbindung verwenden denselben Command-
Builder und dieselbe Umgebungs-Allowlist. Umgebungswerte und Programmargumente
werden nicht in die neuen Belege kopiert. Ein lokaler Besitzlock verhindert
zwei lebende Hostinstanzen mit automatischer Wiederverbindung für denselben
kanonischen Core und dieselbe Node-ID. Shutdown bricht Metadaten-Wartephasen ab und speichert den
beanspruchten Versuch als unterbrochen. Ein gestoppter Host beginnt keine
weiteren Versuche. Der gemeinsame Host-Drop liegt nun beim Host-Lebenszyklus
und beendet alte Nutzer vor Freigabe des Node-Locks.

Die neuen Schemas heißen `arcs.adapter_reconnect.v1` und
`arcs.adapter_reconnect_state.v1`. Ihr Publisher ist für den Core reserviert.
`capabilities_verified: false` kennzeichnet, dass eine passende Selbstbeschreibung
keine zertifizierte Fähigkeit oder geprüfte Modellqualität ist. CLI und
Monitor zeigen aktuelle Core-Belege und lokale Quarantäne-/Handshake-Flags.
Es entsteht weder ein weiterer Manager noch eine zweite Gedächtnisdatenbank.

## Nachweise

| Prüfung | Ergebnis |
| --- | --- |
| ARCS: vollständiger Rust-Workspace einschließlich Doctests | 573 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Binär- und Beispielbuild, Formatierung und `git diff --check` | erfolgreich |
| Clippy, gesamter Workspace und alle Targets | erfolgreich; zwei vorhandene Hinweise in `reasoning/service.rs` |

Der vollständige Rust-Lauf verwendet `cargo test --workspace --locked --offline
--no-fail-fast -- --test-threads=4` in einem separaten Target-Verzeichnis.
Die Runtimes führen ihre Worker dabei weiterhin parallel aus. Sechs neue
Core- und dreizehn neue Prozess-Tests ergänzen die Regression; auch die vier
bestehenden Reservierungstests bestehen nach der Umordnung unverändert in
ihrer fachlichen Prüfung.
Die Python-Prüfung verwendet `python3 -m unittest discover -s
apps/control-panel/tests -v` nach Ende des Rust-Laufs. Die getrennten Adapter
wurden mit ihren vorhandenen Lockfiles offline geprüft. Ihre Quellen und ihr
bisheriger lokaler Veröffentlichungsstatus wurden in diesem Slice nicht
verändert.

Die neuen Tests decken tatsächliche Prozessneustarts, explizite Request-Retries,
unveränderte alte Fehlerbelege, neue wartende Arbeit und unveränderte
gescheiterte Tasks ab. Weitere Fälle prüfen Metadatenabweichungen, endliche
Timeout-Versuche, aktuelle Grants, nicht vertrauenswürdige Belege, konkurrierende
Claims, atomaren Rollback, besetzte Kapazitäten, unabhängige Sensoren,
getrennte Handshake-/RPC-Timeouts und die Umgebungsfreigabe beider Prozesse.
Ein injizierter Publikationsfehler verhindert die Nutzung einer noch nicht
gespeicherten Session. Shutdown unterbricht sogar einen auf eine Stunde
konfigurierten unbeantworteten Handshake, ohne einen weiteren Request an das
Backend zu senden. Eine zweite aktive Hostinstanz wird vor ihrem Prozessstart
abgewiesen; nach sauberem Abbau kann sie die Node übernehmen.

Das [startbare Beispiel](../apps/adapter-host/examples/reasoner-reconnect.md)
wurde mit den normalen externen Solver- und Modell-Fixture-Prozessen in einem
isolierten Verzeichnis ausgeführt. Eine exklusive Sperre auf der Solverdatei
löst einen echten Timeout aus. Nach Freigabe der Datei startet der Host den
Solver neu; der ausdrücklich erlaubte Folge-Request gelingt. Parallel entsteht
ein unabhängiges Ergebnis. Zwei Review-Ergebnisse, ein fehlgeschlagener und
zwei gültige Reasoning-Versuche sowie genau ein automatischer
Wiederverbindungsversuch wurden geprüft. Ein anschließender CLI-Neustart
verwendet genau dieselben drei Versuchsbelege.
Auch drei echte Monitor-Snapshots zeigen die gesunde Session, keine laufende
Quarantäne oder Handshake-Arbeit und dieselben beiden Review-Ergebnisse.

## Grenzen und nächste Arbeiten

Die Funktion setzt einen erfolgreichen ersten Hoststart voraus. Sie behandelt
bekannte Quarantäne im laufenden Host und führt keine Health-Probe oder
Modellinferenz zur Erreichbarkeitsprüfung aus. Anfangs unerreichbare
Installationen und unbeobachtete Ausfälle bisher unbenutzter Sessions bleiben
weitere Arbeit. Ein Metadaten-Handshake beweist keine funktionierende entfernte
Modell-API und keine semantisch richtige Antwort.

Die Policy gilt ausschließlich für reine Reason-Prozesse. Andere Rollen,
physische Aktoren, verteilte Besitzkoordination und Controller-Fencing bleiben
offen. Die vorhandene Verification und Approval gelten unverändert. Ein
verlorener Reasoning-Ausgang braucht weiterhin die bisherige Wiederaufnahme
und Adapter-Deduplizierung. Gelerntes Routing, allgemeine Neuplanung,
Fähigkeitszertifizierung und die universelle Langzeitabnahme sind weitere
Arbeit. Die Beispiele weisen keine echte Modellinferenz, realen Drohnenflug
oder realen mehrmonatigen Dauerbetrieb nach.
