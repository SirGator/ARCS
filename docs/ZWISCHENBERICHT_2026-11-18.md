# ARCS – Zwischenbericht vom 18. November 2026

Die Umsetzung folgt weiterhin der maßgeblichen [ARCHITECTURE.md](ARCHITECTURE.md).
Seit dem letzten veröffentlichten Bericht sind die Slices 10 bis 14 des
[Implementierungsplans](IMPLEMENTATION_PLAN.md) hinzugekommen: gemeinsame
Kapazitätsgrenzen, dauerhafter kooperativer Gerätebesitz, faire Prioritäten,
gesamte Aufgabenlebensdauer und beschriebene Fähigkeiten mit kontrollierter Suche.
Die allgemeinen Phasenabnahmen bleiben offen.

## Architektur und resultierendes Verhalten

### Kapazitäten und exklusive Kontrollsitzungen

[Benannte Kapazitätspools](../apps/adapter-host/examples/resource-pools.md)
koordinieren Reasoning-Aufrufe verschiedener Adapter innerhalb eines Hosts.
Alle beteiligten Adapteranforderungen werden zusammen zugelassen. Ein wartender
Aufruf hält keine Teilreservierung; Abbruch gibt die Plätze erst nach dem
tatsächlichen Ende des Workers frei. Cache-Replay prüft weiterhin Identität und
Rechte, benötigt aber keine zusätzliche Rechenkapazität.

[Exklusive Goal-Ressourcen](../apps/adapter-host/examples/exclusive-goals.md)
liegen als versionierte Artefakte im vorhandenen Core. Auch getrennte Hosts
mit derselben Core-Datenbank koordinieren damit eine Kontrollsitzung über
aufeinanderfolgende Goal-Runden. Vor einer Rundenpause wird die Reservierung
nach bekanntem Abschluss freigegeben; die nächste Runde erhält eine neue Sitzung.

Ein ungeklärter Dispatch bleibt nach Abbruch und Neustart reserviert. Eine
ausdrückliche Klärung verlangt exakte Revisionen, Belegreferenzen und einen
vertrauenswürdigen Anwendungseinstieg. Die Runtime ersetzt weder die
Authentifizierung des Bedieners noch die Prüfung des tatsächlichen Gerätezustands.
Ein abgelaufenes Zeitbudget ist kein Freigabebeleg für einen Aktor.

### Prioritäten und Zeitbudgets

[Dauerhafte Prioritäten](../apps/adapter-host/examples/task-priorities.md)
verteilen Ausführungsversuche mit vier Klassen und Gewichten 1:2:4:8.
Umlaufpositionen und Zähler bleiben im bestehenden Journal erhalten. Begrenzte
Aufgabengruppen verhindern, dass fortlaufend neue Eingaben ältere laufende
Arbeit dauerhaft verdrängen. Die Zusage betrifft die Auswahl innerhalb einer
Runtime, keine hostübergreifende Ressourcenfairness oder garantierte Rechenzeit.

[Zeitbudgets](../apps/adapter-host/examples/task-deadlines.md) begrenzen über
`max_lifetime_ms` die gesamte Lebensdauer einer Aufgabe einschließlich
Wartephasen und Ausfällen. Pläne geben ihre Frist an erzeugte Schritte weiter;
ein engeres Vorlagenbudget kann sie verkürzen. Ein dauerhaft gespeicherter
höchster Zeitstand verhindert eine Verlängerung durch rückwärts gestellte Uhren.

Eine begrenzte Vorprüfung und eine weitere Prüfung vor dem ausgewählten Schritt
beenden fällige Arbeit über die vorhandenen Abbruchpfade. Im Live-Betrieb wird
die Uhr nach langsamem Polling und vor jedem neuen Schritt erneut gelesen.
Explizite Zeitsimulation bleibt über `tick_at` möglich. Planabbruch, offene
Kinder, Joins, Goal-Checkpoints und Ablaufbelege werden atomar gespeichert;
erst danach werden verspätete Worker-Ergebnisse verworfen. Fertige Ergebnisse
und ungeklärter Gerätebesitz bleiben erhalten.

### Beschreibungen und Fähigkeitsauswahl

Der gemeinsame Adaptervertrag unterstützt eine optionale, begrenzte
`CapabilityDescription` mit Zweck, Tags, Voraussetzungen, Wirkungen,
Einschränkungen und Ressourcenangaben. Der bestehende Katalog kennzeichnet sie
ausdrücklich als ungeprüfte Adapterbehauptungen, auch bei hohem Installationstrust.

[Die neue Suche](../apps/adapter-host/examples/capability-discovery.md) kombiniert
lexikalische Relevanz mit harten Filtern für Rolle, Datenverträge, freigegebene
Reasoning-Methode und zulässigen Suchbereich. Bekannte ausgefallene oder interne
Host-Capabilities werden vor der Ergebnisbegrenzung ausgespart. Die Such-CLI
öffnet keine Aufgaben und führt keine Modelle oder Handlungen aus.

Autorisierte Planungsvorlagen erhalten ihre statischen Beschreibungsangaben
in der vorhandenen Reasoning-Anfrage. Ein externer Testplaner wählt damit eine
Vorlage anhand ihrer Beschreibung und verwendet nach einer verlorenen Antwort
denselben Katalog. Ein feindseliger Beschreibungstext kann keine zusätzlichen
Vorlagen oder Rechte autorisieren. Die gebundene Runtime-Definition verhindert
eine stille Änderung des Katalogs bei der Wiederaufnahme.

Alte JSON-Manifeste ohne Beschreibung bleiben unverändert darstellbar. Neue
Beschreibungsfelder benötigen einen aktualisierten Host. Rust-Adapter müssen
ihre Descriptor-Literale um `description: None` oder eine Beschreibung ergänzen.
Die lokalen Drohnen- und Modelladapter sind entsprechend angepasst. Diese
separaten Arbeitsverzeichnisse haben keinen konfigurierten Push-Remote; ihre
lokale Kompatibilitätsprüfung ist vom veröffentlichten ARCS-Repository zu
unterscheiden.
Die Modelladapter-Anpassung ist im separaten lokalen Repository als `1b6d421`
committet. Das Drohnen-Arbeitsverzeichnis hatte bereits vor dieser Änderung
keinen Initialcommit; seine lokale Anpassung ist deshalb kein veröffentlichter
Repository-Stand.

## Verifikation

| Prüfung | Ergebnis |
| --- | --- |
| ARCS: kompletter Rust-Workspace einschließlich SDK-Doctest | 473 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Formatierung und `git diff --check` | erfolgreich |
| Clippy, gesamter Workspace und alle Targets | erfolgreich; zwei vorhandene Hinweise in `reasoning/service.rs` |

Der ARCS-Gesamtlauf verwendet `cargo test --workspace --locked --offline
--no-fail-fast -- --test-threads=4` in einem separaten Target-Verzeichnis.
Vier gleichzeitig ausgeführte Testfälle begrenzen die Konkurrenz der
Prozess-/SQLite-Fixtures; die getesteten Runtimes starten weiterhin ihre
parallelen Worker und konkurrierenden Aufgaben. Im ersten Gesamtlauf fehlte
das neue Rust-Feld noch im SDK-Dokumentationsbeispiel. Das Beispiel wurde
angepasst; der vollständige anschließende Lauf ist grün.

Der Binärbuild erfolgt mit `cargo build --workspace --bins --locked --offline`.
Die Python-Prüfung verwendet `python3 -m unittest discover -s
apps/control-panel/tests -v`. Die beiden separaten Rust-Anbindungen wurden
ebenfalls offline mit ihren vorhandenen Lockfiles getestet.

Zusätzlich ist das startbare Suchbeispiel mit echten Adapterprozessen in einem
isolierten Verzeichnis geprüft: Der Solver wird anhand von Suchwort, Rolle,
Schema und Methode gefunden, die Beschreibung bleibt als ungeprüft markiert,
der Wiederholungsstart liefert dasselbe Ergebnis und es entstehen weder
Core-Task-Eingaben noch Reasoning- oder Aktionsaufrufe. Die vorhandenen
Prozess-Tests prüfen Gerätebesitz, Monatswartephasen, Budgetablauf und Replay
einschließlich ihrer negativen und unterbrochenen Abläufe.

## Nachweise und Grenzen

Die neuen Beispiele verwenden deterministische Rules-Reasoner, Modell-Fixtures
und simulierte Geräte. Es wurde damit weder die Leistungsfähigkeit eines echten
14B-Modells noch ein autonomer Flug nachgewiesen. Die lexikalische Suche bestätigt
keine Voraussetzungen, Wirkungen, gemessene Zuverlässigkeit oder wirtschaftliche
Eignung eines Werkzeugs.

Die Änderungen erhalten einen logischen Core, das gemeinsame Artifact-Netz
und die bestehenden Manager- und Berechtigungsgrenzen. Die zusätzliche
Aufgabensteuerung nutzt das bestehende operative Journal; sie führt keine zweite
Gedächtnisdatenbank oder übergeordnete Agenteninstanz ein.

Weiter offen sind insbesondere:

- Autonomes Routing mit tatsächlicher Modelleignung, Kosten und Zuverlässigkeit.
- Allgemeine Zielzerlegung, versionierte Neuplanung und freie Werkzeugkomposition.
- Semantisches Artifact-Retrieval, Konsolidierung und umfassende Abrufbenchmarks.
- Pause/Resume laufender Pläne sowie allgemeine Token-, Geld- und API-Budgets.
- Durchgehend parallele Request-, Act- und Output-I/O und harte Prozessisolation.
- Gerätecontroller mit Besitzprüfung/Fencing, sichere Übergabe und Echtzeitabbruch.
- Längere reale Betriebserprobung und die vollständigen universellen Systemtests.

Der nächste Schwerpunkt ist die tatsächliche Auswahl geeigneter Reasoner und
Werkzeuge innerhalb dieser Verträge, mit persistentem Auswahlbeleg und einem
nachgewiesenen Ergebnis statt einer bloßen Auswahlbehauptung.
