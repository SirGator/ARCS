# ARCS – Zwischenbericht zur Textsuche, 18. November 2026

Dieser Bericht ergänzt den [Stand bis Slice 14](ZWISCHENBERICHT_2026-11-18.md)
und die [Reasoner-Auswahl aus Slice 15](ZWISCHENBERICHT_2026-11-18_ROUTING.md).
Die maßgebliche [ARCHITECTURE.md](ARCHITECTURE.md) bleibt unverändert.
Slice 16 setzt weitere Teile von v3 §6.5 MEM-02/MEM-08/MEM-09/MEM-10 um.

## Ergebnis

Memory-Scopes können jetzt freigegebene Stringfelder lexikalisch durchsuchen.
So findet ARCS eine ältere relevante Erinnerung auch dann, wenn jüngere
unpassende Notizen das bisherige Metadatenfenster füllen. Suchtext kann statisch
konfiguriert oder aus ausdrücklich benannten internen Eingabefeldern ergänzt
werden. Das erweitert den Datenexport nicht; maßgeblich bleiben die
vorhandenen `payload_fields`.

Schema, Subject und Tags begrenzen die Auswahl vor der Trefferbegrenzung.
Graphnachbarn müssen denselben Textfilter erfüllen. Bei überlappenden Scopes
stammt die exportierte Feldauswahl aus einem tatsächlich passenden Scope.
Felder anderer Projekte oder private, nicht freigegebene Textfelder können
keine öffentlichen Treffer erzeugen. Fehlende oder leere Anfragefelder sind
Aufgabenfehler; eine gültige Suche ohne Treffer liefert keine zusätzliche
Erinnerung und fällt nicht auf einen ungefilterten Abruf zurück.

Der normalisierte Textindex liegt als abgeleitete Feldprojektion in derselben
SQLite-Datei wie die Originalartefakte. Er ist weder eine zweite Gedächtnis-
Wahrheitsquelle noch ein eigener Agent. Beim Öffnen einer alten Datenbank
entsteht kein vollständiger Payloadscan. Erst ein benötigter Abruf baut den
Index in begrenzten Stapeln auf; Fortschritt und Einträge committen gemeinsam.
Getrennte Verbindungen koordinieren denselben Aufbau atomar.

Solange eine benötigte Projektion unvollständig ist, wartet die Aufgabe mit
`memory-text-index`. Sie hält keinen Reasoning-Worker und speichert keinen
irreführenden Teilkontext. Unabhängige Aufgaben können weiterlaufen. Zusätzlich
zu bestehenden Graph- und Kontextbudgets begrenzen Artefaktgröße, Stapelgröße,
Projektionszahl und eine SQLite-VM-Schrittgrenze die neue Arbeit. Die Schritt-
grenze ist keine harte Wanduhr- oder Betriebssystem-I/O-Garantie.

Der endgültige Kontext bleibt als `arcs.context_selection.v1` eingefroren.
Neustarts und verlorene Abschluss-Checkpoints verwenden denselben Beleg,
selbst wenn der abgeleitete Index inzwischen gelöscht wurde. Der Store-Einstieg
`clear_text_indexes()` entfernt ausschließlich Projektionen; neue Suchanfragen
bauen sie wieder auf. Gespeicherte Originale, Trust und Beziehungen bleiben
erhalten. Neue Belege beeinflussen erst spätere Requests.

## Architektur und Kompatibilität

Die Zuständigkeiten sind getrennt: lexikalische Normalisierung und Suchscope
liegen beim Retrieval, Indexaufbau und SQL beim vorhandenen Store, interne
Anfragefelder und Exportminimierung bei der Kontextbildung. Der Host übersetzt
unvollständigen Aufbau in das bestehende Ressourcenwarten. Das operative
Aufgabenjournal benötigt keine neue Autoritäts- oder Gedächtnistabelle.

Bestehende Memory-JSON-Konfigurationen behalten ihre Identität und benötigen
keine Änderung. Das unveränderte Kontextartefakt-Schema nimmt die optionalen
Suchangaben in der bestehenden Policy und Indexstände im bestehenden Trace auf.
Rust-Aufrufer verwenden `RetrievalQuery.scopes` mit `RetrievalScope`, der den
bisherigen Metadatenfilter und einen optionalen Textfilter zusammenführt.
`MemoryScope` ergänzt das optionale Feld `text_query`.

## Nachweise

| Prüfung | Ergebnis |
| --- | --- |
| ARCS: vollständiger Rust-Workspace einschließlich Doctests | 506 bestanden, 0 fehlgeschlagen |
| Kontrollpanel: Python-Unittests | 29 bestanden, 11 Desktop-Tests ohne grafische Sitzung übersprungen |
| Separater Drohnen-Workspace | 33 bestanden, 0 fehlgeschlagen |
| Separater Modelladapter | 3 bestanden, 0 fehlgeschlagen |
| Binärbuild, Formatierung und `git diff --check` | erfolgreich |
| Clippy, gesamter Workspace und alle Targets | erfolgreich; zwei vorhandene Hinweise in `reasoning/service.rs` |

Der Rust-Gesamtlauf verwendet `cargo test --workspace --locked --offline
--no-fail-fast -- --test-threads=4` in einem separaten Target-Verzeichnis.
Die Worker innerhalb der getesteten Runtimes bleiben parallel. Die Python-
Prüfung verwendet `python3 -m unittest discover -s apps/control-panel/tests -v`.
Die separaten lokalen Rust-Anbindungen wurden offline mit vorhandenen Lockfiles
geprüft; ihre Quellen wurden in diesem Slice nicht geändert. Ihr im vorherigen
Bericht dokumentierter lokaler Veröffentlichungsstatus bleibt bestehen.

Ein zunächst gleichzeitig mit mehreren Builds und Rust-Prozessprüfungen
gestarteter Kontrollpanel-Lauf verfehlte einmal die Eingabe-Startfrist; drei
Drohnen-Kameratests verfehlten ihre Adapter-Startfrist. Nach dem ARCS-Gesamtlauf
wurden Kontrollpanel und Drohnen-Workspace einzeln vollständig erneut geprüft.
Beide Läufe bestanden mit unveränderten Timeouts. Die Startfehler traten damit
nur im konkurrierenden Lauf auf; ihre genaue Ursache wurde nicht separat
profiliert. Eine allgemeine Last- oder Latenzabnahme bleibt offen.

Elf neue Core-Tests und fünf Prozess-Tests decken ältere Treffer, negative
Suchdaten, Projekt-/Feldgrenzen, Graphpfade, Unicode-Normalisierung,
übergroße Versionen, SQLite-Arbeitsabbruch, konkurrierende Verbindungen,
Indexneubau, überlappende Scopes und Wiederaufnahme ab.

Ein gezielter Regressionstest enthält eine ältere relevante Notiz, 100 jüngere
unpassende Notizen und einen gleichlautenden fremden Projektbeleg. Bei derselben
Grenze von drei besuchten Versionen gilt:

| Verfahren | Relevanter Beleg gefunden | Fremde Projektbelege |
| --- | --- | --- |
| Bisheriger Metadaten-/Graphabruf | 0 von 1 | 0 |
| Lexikalischer Textabruf | 1 von 1 | 0 |

Das [startbare Beispiel](../apps/adapter-host/examples/text-memory.md) wurde mit
echten Adapterprozessen in einem isolierten Verzeichnis geprüft. Es findet die
ältere Kalibrierungsnotiz, exportiert nur deren `body` und erreicht einen
Kandidaten zur Prüfung. Wiederholungsstart und vier Monitor-Snapshots behalten
den vorhandenen Kontext; der Solver wird insgesamt einmal aufgerufen. Ein
separater Baseline-Auftrag mit denselben Besuchsgrenzen und ohne Textfilter
verfehlt die ältere relevante Notiz.

## Grenzen und nächste Arbeiten

Die Suche ist lexikalisch mit vollständigen Wörtern und AND-Verknüpfung. Sie
bietet keine Bedeutungsähnlichkeit, Stammbildung oder gelernte Rangfolge.
Historische Treffer sind keine Bestätigung ihrer heutigen Gültigkeit oder
fachlichen Wahrheit. Temporalität, Konsolidierung, breitere Qualitätsbenchmarks,
Tokenersparnis und gemessene Antwortqualität bleiben offen.

Die Beispiel-Reasoner bleiben deterministische Solver oder Test-Fixtures.
Es wurde kein echter 14B-Modellvorteil, autonomer Flug oder monatelanger realer
Betrieb nachgewiesen. Die neuen Indizes sind begrenzt und wiederaufbaubar;
Kapazitätsüberschreitungen und zu aufwendige Suchanfragen werden ausdrücklich
abgewiesen. Die allgemeinen Architekturphasen sind weiterhin nicht abgenommen.
