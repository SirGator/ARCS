# ARCS – Implementierungsplan v2.0

> Arbeitsgrundlage vom 15. November 2026. Die Checkboxen beschreiben die
> Gesamtabnahme der jeweiligen Aufgabe, nicht nur erste Teilimplementierungen.
> Architekturvertrag: [ARCHITECTURE.md](ARCHITECTURE.md).
> Gesamtziel und künftige Anforderungen:
> [Technische Gesamtarchitektur v3.0](ARCHITECTURE.md).
> Anforderungs-IDs bleiben versions- und abschnittsbezogen; die bestehenden
> v2-Aufgaben und Abnahmestände werden durch v3 nicht umnummeriert.
>
> Ergänzung vom 27. November 2026:
> [Zielbilderweiterung v3.1](ZIELBILD_ERWEITERUNG_2026-11-27.md) ergänzt das
> Zielbild um Unified Multi-Context Runtime, Datenschutz und
> Informationsflusskontrolle, formale Methoden, dauerhaften Betrieb und
> Unternehmensfähigkeit. Die neuen Anforderungen `CTX-*`, `PRIV-*`, `FORM-*`,
> `OPS-*` und `BIZ-*` sind in den Phasen EXT-0 sowie EXT-CTX bis EXT-BIZ als
> Arbeitspakete festgehalten und noch nicht abgenommen. Die Phasen EXT-0 bis
> EXT-BIZ bauen auf den bestehenden
> Verträgen aus Phase 1 bis Phase 8 auf und ersetzen sie nicht. EXT-0 ist
> der vorgeschaltete Abgleich, weil die Erweiterung diese Verträge erweitert
> statt eine Ersatzarchitektur aufzubauen.
> Bestehende v2- und v3-Aufgaben werden nicht umnummeriert und nicht als
> erledigt markiert. Die `OPS-01` bis `OPS-06` der Erweiterung sind **nicht**
> die `OPS-01`/`OPS-02` aus Phase 8 und **nicht** `OPS-01` aus v3 §12.
>
> Dritter Slice: Schutzstufen mit erzwungener Ableitung und Default-Deny an
> der Offenlegungskante. Artefakte tragen ein Sicherheitsprofil aus Deklaration
> und wirksamer Stufe; der Store löst die Stufe aus Provenienz und Vorgängerversion
> auf und lehnt unbelegte Herabstufungen ab. Abruf, Memory-Scope und
> Adapter-Grant begrenzen die Datenklasse vor der Offenlegung. `PRIV-01`,
> `PRIV-02` und `PRIV-03` sind damit teilweise umgesetzt, `PRIV-07` in der
> Datenklassenprüfung. Secret-Broker, Egress-Isolation, Löschung, Zweckbindung
> und Vertragsprüfung im Routing bleiben offen. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_SCHUTZSTUFEN.md).
>
> Fünfter Slice der Erweiterung: formale Nachweise mit einer isolierten
> Lean-Engine als externem Adapter. Der Core führt formale Aussagen,
> Beweisobligationen, Beweise, Gegenbeispiele und Verifikationsberichte als
> versionierte Artefakte und entscheidet, ob ein Nachweis eine Handlung
> trägt. Lean selbst bleibt ein Werkzeug: Es erbt weder Autorität noch
> Store-Zugriff. Ein vorgeschriebener Nachweis entscheidet vor dem übrigen
> Handlungsvertrag; fehlt er, wartet das Ziel, statt zu handeln. `FORM-01`,
> `FORM-02` und `FORM-05` sind damit umgesetzt, `FORM-03` in der Grundlage.
> Offen bleiben `FORM-04`, `FORM-06` und `FORM-07`. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_FORMALE_NACHWEISE.md).
>
> Sechster Slice der Erweiterung: Aufgaben-Leases und das Buch der externen
> Wirkungen. Eine Aufgabe wird jetzt dauerhaft einer logischen Runtime
> zugeordnet; das Fencing-Token macht den Besitzwechsel eindeutig, und ein
> fremder Eigentümer führt die Aufgabe in einen Wartezustand statt in eine
> konkurrierende Bearbeitung. Jede externe Wirkung wird als Artefakt mit
> Operation, Aufgabe, Invocation, Zustand und Versuchszahl gebucht. Der Core
> unterscheidet „nicht eingetreten“ (ein neuer Versuch ist erlaubt), „belegt
> eingetreten“ (nur der Beleg wird nachgeholt) und „nicht zuordenbar“ (eine
> ausdrückliche Klärung des Betreibers ist nötig). `OPS-03` ist damit in der
> Grundlage umgesetzt, `OPS-05` für Aufgaben, Wirkungen und Freigaben,
> `TEST-10` für die Zuordnung, die ausbleibende doppelte Wirkung und die
> ausdrückliche Klärung nachgewiesen. Offen bleiben `OPS-01`, `OPS-02`,
> `OPS-04`, `OPS-06` sowie `BIZ-01` bis `BIZ-06`. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_AUFGABEN_LEASES.md).
>
> Achter Slice der Erweiterung: Abstraktion, symbolisches Reasoning und
> lokale Rekonstruktion. Der Core zerlegt einen vertraulichen Sachverhalt
> lokal in Variablen, prüft die Abstraktion auf verbliebene Offenlegung und
> gibt nur das abstrakte Problem an den freigegebenen Reasoner frei. Die
> Zuordnung von Platzhalter zu Wert bleibt im Core, und die konkrete Lösung
> wird lokal rekonstruiert, geprüft und mit der Schutzstufe der Quelle
> gespeichert. Ergänzend prüft die v2-Reasoning-Kante die Schutzstufe jedes
> Kontextartefakts selbst und die Freigabe des Empfängers; die v1-Kante
> prüft den Empfänger ebenfalls. Damit sind `PRIV-08` in der Grundlage,
> `PRIV-09` für die Nutzlast und `PRIV-10` für die Rekonstruktion
> umgesetzt, `PRIV-02` an beiden Kanten nachgezogen. Offen bleiben
> `PRIV-06`, `PRIV-09` für erzeugten Code, `PRIV-10` für die formale
> Prüfung der Anwendbarkeit, `PRIV-11` sowie die Messung der Baselines aus
> der Forschungsfrage. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_ABSTRAKTION.md).
>
> Siebter Slice der Erweiterung: vertrauliche Fähigkeiten. Geheimnisse
> liegen in einem Tresor außerhalb des Artefaktnetzes und werden nur über
> auditierbare Freigaben vermittelt, die an Zweck, Ressource, Ablaufzeit und
> Nutzungszahl gebunden sind. Der gespeicherte Auftrag behält die Referenz;
> der Fingerabdruck der Invocation entsteht ohne eingesetztes Geheimnis, und
> eine Antwort, die einen Wert zurückspiegelt, wird vor dem Speichern
> redigiert. Eine Egress-Grenze im Adapter-Grant legt fest, wohin Daten einer
> Installation gelangen dürfen; ein Auftrag kann ein Ziel benennen, aber
> seine Reichweite nicht erweitern. `PRIV-04` ist damit in der Grundlage
> umgesetzt, `PRIV-05` für die vom Core kontrollierten Grenzen. Offen
> bleiben `PRIV-06`, `PRIV-08` bis `PRIV-11` und die Sandbox des
> Kindprozesses. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_VERTRAULICHE_FAEHIGKEITEN.md).
>
> Vierter Slice der Erweiterung: vereinte Multi-Context-Runtime mit
> persistenten Arbeitskontexten. Jede bearbeitete Aufgabe erhält einen
> leichtgewichtigen Arbeitskontext im Core, der Ziel, Task, Planrevision,
> Artefaktversionen, Authority-Scope, Memory-Sicht, Priorität, Budget und
> die laufenden Operationen bindet, ohne eine eigene Agenteninstanz zu
> starten. Das Ergebnis wird als Operation gebunden, ein begrenzter
> Statusbericht nennt Zustände und Bindungen ohne Nutzdaten. `CTX-01`,
> `CTX-03` und `CTX-05` sind damit teilweise umgesetzt, `CTX-09` in der
> Grundlage. Offen bleiben `CTX-02`, `CTX-04`, `CTX-06`, `CTX-07`, `CTX-08`,
> `CTX-10` und `CTX-11`. Nachweis:
> [Zwischenbericht 28.11.2026](ZWISCHENBERICHT_2026-11-28_ARBEITSKONTEXTE.md).
>
> Erster umgesetzter Slice: begrenztes paralleles Reasoning mit zentraler
> Ergebnisvalidierung, exklusiven Adapter-Sessions, Fehlerisolierung und
> Wiederaufnahme. PAR-01, PAR-03, PAR-04, PAR-05, PAR-06 und PAR-07 sind damit teilweise
> umgesetzt; ihre vollständige allgemeine Abnahme bleibt offen. ARC-01 bis
> ARC-03 sind dokumentiert und vorhandenen Diensten zugeordnet. Noch offen sind
> insbesondere parallele Act-/Request-/Output-Ports, Planverzweigungen,
> allgemeine Ressourcenreservierungen und Langzeitscheduling.
>
> Zweiter Slice: begrenzter Graph-/Metadatenabruf und automatische Kontextbildung
> innerhalb ausdrücklich erlaubter Memory-Scopes. Die Auswahl wird im Core als
> Artefakt eingefroren und bei Wiederaufnahme unverändert verwendet. MEM-02,
> MEM-03, MEM-04, MEM-07 und MEM-08 sind damit teilweise umgesetzt. Semantische
> Suche, automatische Konsolidierung und allgemeine Abrufqualitätsbenchmarks
> bleiben offen. Reason-Anbindung, Aufgabenverarbeitung, Journal-SQL und
> Graph-Persistenz wurden entsprechend ihren Zuständigkeiten getrennt.
>
> Dritter Slice gemäß der maßgeblichen ARCHITECTURE.md, v3 §10.1 SEC-05:
> Der allgemeine Host begrenzt die geerbte Prozessumgebung pro Adapter.
> Die Modellkonfiguration erzeugt ausdrückliche Schlüsselfreigaben pro Rolle;
> Verbindungstests und lokale Eingabeprozesse übernehmen keine fremden
> Modellschlüssel. Prozess-, Neustart- und Integrationstests sichern diese
> Teilimplementierung ab. Eine Betriebssystem-Sandbox gemäß SEC-06 und die
> vollständige Abnahme von SEC-05 bleiben offen.
>
> Vierter Slice gemäß v3 §7.2 PAR-06 sowie §8.2 LNG-01, LNG-03 und LNG-04:
> Persistente einmalige Termin-/Ereignisaktivierung vor dem ersten Schritt,
> eingefrorene relative Fälligkeit und begrenzt paginierte Ereignissuche mit
> dauerhafter Umlaufposition. Eine volle Queue empfängt weiterhin ihre
> Weckereignisse. Die Input-Abholung wurde dazu als eigene Host-Zuständigkeit
> ausgegliedert. Prozess-Tests prüfen simulierte Monate, Recovery und
> unveränderte manuelle Aktionsfreigaben. Wiederkehrende Termine, dynamisches
> Warten zwischen Planschritten und das allgemeine Langzeit-Zielmodell bleiben
> offen; die v2-Phase LNG ist damit teilweise umgesetzt, nicht abgenommen.
>
> Fünfter Slice gemäß v3 §8.2 LNG-03 bis LNG-07: Goal-Routen können über
> `between_rounds` wiederholt zwischen vollständig bewerteten Runden warten.
> Jeder Termin oder Ereigniswartepunkt erhält einen eigenen Aktivierungsbeleg;
> die Migration erhält vorhandene Belege als Revision 0. Nach dem Wecken
> folgen frische Datenerfassung, optional ein neuer Zielcheck und erneute
> Verification/Approval. Abbruch und Rundenbudget bleiben wirksam. Prozess-Tests
> decken simulierte Monate, Memory, neue Freigaben und Fehler beim Warte-Commit
> nach bereits erfolgter Gerätewirkung ab. Zielablauf, Warteübergänge und
> Scheduler-Migration sind nach Zuständigkeit getrennt. Dynamisch vorgeschlagene
> Wartepläne, Unterbrechungen innerhalb einer Runde und das allgemeine
> versionierte Langzeit-Zielmodell bleiben offen.
>
> Sechster Slice gemäß v3 §5.2 HET-03/HET-04/HET-09, §5.3 und §7.2 PAR-03:
> Feste Routenabhängigkeiten derselben Core-Eingabe ermöglichen parallele
> Zweige und anschließende All-Joins. Graphprüfung, atomare Aufnahme,
> persistente Ergebnisbindung und begrenzte Prüfung wartender Nachfolger sind
> implementiert. Fehlgeschlagene Voraussetzungen blockieren nachfolgende
> Aufträge; neue Handlungen brauchen weiterhin eigene Verification/Approval.
> Ausgewählte Felder, unveränderte Versionsbezüge, Memory und Recovery werden
> mit Prozess-Tests geprüft. Dynamische Planartefakte, automatische Teilziele,
> Neuplanung und eingabeübergreifende Abhängigkeiten bleiben offen.
>
> Siebter Slice gemäß v3 §5.2 HET-01/HET-02/HET-03/HET-04/HET-08/HET-09,
> §5.3 und §8.2 LNG-02: Externe Reasoner können begrenzte Reasoning-DAGs aus
> Betreiber-Vorlagen vorschlagen. Vor Ausführung werden Vorlagen, Exporte,
> Abhängigkeiten und Gesamtbudgets geprüft. Angenommene Pläne bleiben als
> `arcs.plan.v1` mit eingefrorenen Routen im Artifact-Netz erhalten. Vorab
> reservierte Queuekapazität verhindert eine blockierte Planerweiterung;
> die Aufnahme aller Schritte erfolgt atomar. Prozess-Tests decken ungültige
> Pläne, parallele Zweige, volle Kapazität, Wiederaufnahme, private Constraints
> und monatliche Wartephasen ab. Dynamische Goal-/Act-Schritte, freie
> Werkzeugargumente, Neuplanung und allgemeiner Planabbruch bleiben offen.
>
> Achter Slice gemäß v3 §7.2 PAR-07 und §8.2 LNG-05: Reasoning-Teilpläne
> lassen sich einschließlich eines noch wartenden Planners gezielt abbrechen.
> Abbruchbeleg, offene Kindaufträge und wartende Joins werden atomar gespeichert;
> fertige Review-Ergebnisse bleiben erhalten. Späte Antworten werden ohne
> Core-Ergebnisübernahme abgeholt, während laufende Worker bis Antwort oder
> Deadline ihre Ressourcenplätze behalten. Unabhängige Pläne und nachfolgende
> Aufträge derselben rechtzeitig antwortenden Adapter-Session bleiben nutzbar.
> Die CLI bietet `--plan-command`; Prozess-Tests prüfen Abbruch, Rollback,
> Neustart, additive Journalmigration, Instanzisolation und Deadlines. Ein
> allgemeines Core-Zielmodell, Pause/Resume laufender Pläne und ein Live-IPC-
> Steuerkanal bleiben offen; LNG-05 ist damit nur teilweise umgesetzt.
>
> Neunter Slice gemäß v3 §5.2 HET-03/HET-04/HET-09, §5.3, §8.2 LNG-02/LNG-05
> und §10.1 SEC-01/SEC-03: Erzeugte Pläne können freigegebene Goal-Vorlagen
> instanziieren. Ihre Aktionen durchlaufen die vorhandene Verification und
> zwingend manuelle Freigaben; der Root-Plan erhält keine Action-Autorität.
> Gesamtbudgets berücksichtigen alle maximalen Runden, Planungs-/Assessment-
> Aufrufe und Action-Dispatches. Joins verwenden erst den abschließenden
> Goal-Assessment-Beleg. Request-Publisher werden pro Betreiber-Vorlage
> registriert; Plan- und Goal-Abbruch stoppen auch aktive Goal-Reasoning-Phasen.
> Prozess-Tests prüfen Freigaben, Ablehnung, Wirkung, Ergebnisexport, Neustart,
> unterbrochene Ausführungs-Checkpoints und Monatswartephasen bei voller Queue.
> Ein Rules-Beispiel verbindet die dynamische Auswahl mit einem simulierten
> Gerät. Automatische Freigaben in erzeugten Plänen, direkte Act-Knoten,
> freie Argumente, universelle Zielzerlegung und Neuplanung bleiben offen.
>
> Zehnter Slice gemäß v3 §7.2 PAR-04/PAR-11 und §7.3: Host-Konfigurationen
> können gemeinsame benannte Kapazitätspools und Anforderungen pro beteiligtem
> Reasoning-Adapter deklarieren. Zulassung berücksichtigt alle Kapazitäten
> gemeinsam; wartende Aufrufe halten keine Teilreservierungen. Belegung wird
> aus ausstehenden Worker-Aufrufen abgeleitet und über Fehler, Timeout und
> Abbruch korrekt freigegeben. Zwei Runtimes desselben Hosts teilen die Grenzen;
> auch synchrone Aufrufe beachten laufende Worker. Validiertes Cache-Replay
> benötigt keine neuen Kapazitäten. Prozess-Tests prüfen neun Ressourcen-
> Szenarien; Monitor und Host-Bericht zeigen die Belegung. Das ist eine
> Zulassungskontrolle für Reason V2 innerhalb eines Host-Prozesses. Physische
> Geräte-Leases, Koordination mehrerer Hosts, Controller-Fencing, harte GPU-/
> RAM-Grenzen und die vollständige Abnahme von PAR-04 bleiben offen.
>
> Elfter Slice gemäß v3 §7.2 PAR-04/PAR-10 und §7.3: Goal-Routen und
> freigegebene Goal-Vorlagen können exklusive Ressourcen für ihre Kontrollsitzung
> deklarieren. Besitz und Freigabe liegen als versionierte Core-Artefakte mit
> atomaren Mehrfachänderungen und Revisionsprüfung vor. Kooperierende Hosts
> mit derselben Core-Datenbank teilen diese Reservierungen. Fortlaufende Runden
> behalten sie, Rundenpausen geben sie nach einem passenden Abschluss frei.
> Ungeklärte Dispatches bleiben auch nach Abbruch und Neustart reserviert;
> terminale Goals können nach authentifizierter Klärung mit exakten Revisionen
> und Belegreferenzen freigegeben werden. Vier Core- und neun Prozess-Tests
> prüfen Konkurrenz, Rollback, Neustart, Monatswartephasen und Freigaben.
> Automatischer Lease-Ablauf, sichere Controller-Übergabe/Fencing und die
> Absicherung nicht kooperierender Act-Clients bleiben offen.

> Zwölfter Slice gemäß v3 §7.2 PAR-05/PAR-10/PAR-12: Betreiber-Routen und
> freigegebene Vorlagen können Prioritäten 0..3 festlegen. Ein gewichteter
> Klassenumlauf mit 1:2:4:8 Plätzen und dauerhaften Aufgabenzeigern ersetzt den
> flüchtigen Auswahlcursor. Eingefrorene Gruppengrenzen verhindern, dass stetige
> neue Eingaben ältere laufende Aufgaben verdrängen. Termin-, Ereignis- und
> Abhängigkeitswartephasen werden nicht ausgewählt; Freigaben werden weiterhin
> gegen den Core abgeglichen. Pro Tick bleibt jeder Auftrag auf einen Versuch begrenzt.
> Auswahlmetadaten werden vor I/O atomar im vorhandenen Journal gespeichert;
> CLI und Monitor zeigen dauerhafte Zähler je Klasse. Bestehende Jobs erhalten
> ihre Prioritäten additiv aus festen Routen oder autorisierten Vorlagen, ohne
> Core-Ergebnisse neu zu erzeugen. Vier Scheduler- und fünf Prozess-Tests prüfen
> Gewichte, fortlaufende Eingaben, Rollback, Neustarts, Migration und Modellgrenzen.
> Die Zusage betrifft Auswahlversuche innerhalb einer Runtime; Ressourcenfairness
> über Runtimes/Hosts, Preemption und vollständige Latenz-/Durchsatzmessungen bleiben
> offen. PAR-05 und PAR-12 sind damit teilweise umgesetzt, nicht abgenommen.

> Dreizehnter Slice gemäß v3 §7.2 PAR-07/PAR-10 und §8.2 LNG-01/LNG-06:
> Feste Routen und autorisierte Vorlagen können mit `max_lifetime_ms` eine
> gesamte Lebensdauer einschließlich Wartephasen und Ausfällen begrenzen.
> Annahme- und Ablaufzeiten werden atomar mit dem Auftrag gespeichert;
> Planschritte übernehmen die frühere eigene oder elterliche Frist. Ein
> dauerhafter höchster Zeitstand verhindert eine Verlängerung durch rückwärts
> gestellte Uhren. Begrenzte Vorprüfung und zusätzliche Schrittprüfung stoppen
> fällige Arbeit über die vorhandenen Abbruchpfade. Plan-/Goal-Checkpoints,
> wartende Joins und Ablaufbelege committen gemeinsam; erst danach werden
> Worker-Antworten verworfen. Fertige Ergebnisse und ungeklärte Gerätebesitzstände
> bleiben erhalten. CLI und Monitor zeigen Fristen, Belege und Ablaufzähler.
> Elf Prozess-Tests prüfen Monats-Recovery, Live-Zeit nach langsamer Beobachtung,
> Vererbung, kürzere Teilbudgets,
> blockierte Pläne, Rollback, Kapazitäten, Freigaben und Gerätewirkung.
> Harte Echtzeitunterbrechung, Pause/Resume, Token-/Geldbudgets und die vollständige
> Abnahme von PAR-07 und LNG-06 bleiben offen.

> Vierzehnter Slice gemäß v3 §9.1/§9.2 und v2 CAP-01/CAP-02: Optionale,
> begrenzte Capability-Beschreibungen erweitern den gemeinsamen Adaptervertrag.
> Katalog und Planungskontext kennzeichnen diese Angaben unabhängig vom
> Installationstrust als ungeprüfte Adapterbehauptungen. Die vorhandene Registry
> bietet lexikalische Suche mit harten Rollen-, Schema-, Methoden- und Scope-
> Filtern. Der Host schließt interne und bekannte ausgefallene Installationen
> vor der Ergebnisbegrenzung aus. Die CLI validiert Suchanfragen vor dem
> Verbindungsaufbau und führt keine Task-Operationen aus. Autorisierte Vorlagen
> erhalten statische Beschreibungsangaben im bestehenden Planer-Kontext;
> gebundene Manifestidentitäten sichern Replay und Drift-Erkennung ab.
> Zwei Protokoll-, vier Core- und sechs Prozess-Tests prüfen Grenzen,
> Ranking/Grants, bekannte Ausfälle, CLI, beschreibungsabhängige Auswahl,
> Wiederaufnahme und feindselige Angaben. Kompatible Defaults für vorhandene
> JSON-Manifeste sowie angepasste Rust-Literale erhalten die lokalen Anbindungen.
> Semantische Suche, autonome Auswahl mit gemessener Zuverlässigkeit/Kosten,
> automatische Fähigkeitsprüfung und allgemeine freie Werkzeugkomposition
> bleiben offen; CAP-01/CAP-02 sind teilweise umgesetzt, nicht abgenommen.

> Fünfzehnter Slice gemäß v3 §9.2/§9.3 und §7.2 PAR-04/PAR-10: Betreiber-
> Routen und autorisierte Vorlagen können mehrere erlaubte Reasoner mit eigenen
> Methoden und Budgets anbieten. Eine begrenzte lexikalische Suche kombiniert
> Relevanz, Betreiberreihenfolge und aktuelle Host-Verfügbarkeit. Der Core friert
> die Auswahl vor dem Dispatch als Artefakt ein; konkurrierende Verbindungen und
> Wiederaufnahme übernehmen denselben Beleg. Private Eingabefelder können die
> Suche steuern, ohne zusätzlichen externen Kontext zu erzeugen. Goals binden
> erst bei tatsächlichem Planungsbedarf; Assessor, Verification und manuelle
> Freigaben bleiben separat. Root-Pläne, erzeugte Schritte und Joins prüfen den
> tatsächlichen Produzenten; Gesamtbudgets decken alle erlaubten Optionen ab.
> Fünf Core- und zwölf Prozess-Tests prüfen Auswahl, Fehlerisolierung, Recovery,
> konkurrierende Speicherung, Cache-Replay und bestätigte simulierte Wirkung.
> Verfügbarkeit ist eine Beobachtung, keine Ressourcenreservierung. Allgemeine
> versionierte Neuplanung, gemessene Eignung/Zuverlässigkeit, Kostenoptimierung
> und freie Werkzeugauswahl bleiben offen; die allgemeinen Abnahmen bleiben
> unverändert offen.

> Sechzehnter Slice gemäß v3 §6.5 MEM-02/MEM-08/MEM-09/MEM-10: Memory-Scopes
> können freigegebene Stringfelder lexikalisch durchsuchen. Explizite interne
> Eingabefelder können die Anfrage ergänzen, ohne zusätzlichen Datenexport.
> Derselbe SQLite-Store hält begrenzte, wiederaufbaubare Feldprojektionen;
> Aufbaufortschritt und Indexeinträge committen atomar. Unvollständiger Aufbau
> führt zu Ressourcenwarten ohne Worker oder eingefrorenen Teilkontext. Die
> Suchausführung erhält zusätzlich eine SQLite-VM-Arbeitsgrenze. Alte passende
> Belege werden innerhalb derselben Besuchsgrenze gefunden, wo die jüngste
> Metadatenstichprobe sie verfehlt. Projektgrenzen, Graphfilter, Feldminimierung,
> Trust und bestehende Kontextbudgets bleiben wirksam. Elf neue Core- und fünf
> Prozess-Tests prüfen Abruf, negative Fälle, Konkurrenz, Indexneubau und
> Wiederaufnahme auch nach Indexlöschung. Semantische Suche, Konsolidierung,
> zeitliche Wissensgültigkeit, breite Qualitäts-/Token-/Latenzbenchmarks und
> die allgemeinen Phasenabnahmen bleiben offen.

> Siebzehnter Slice gemäß v3 §8.2 LNG-05 und §7.2 PAR-05/PAR-07/PAR-10:
> Revisionierte, idempotente Betreiberkommandos pausieren Aufgaben und komplette
> Pläne im bestehenden Journal. Der Scheduler sperrt auch erzeugte Kinder;
> eigene Kindpausen bleiben unabhängig. Vorhandene Reasoning-Worker werden
> ohne neuen Dispatch abgeholt, validiert und freigegeben. Wiederaufnahme nutzt
> persistierte Ergebnisse und verhindert nach verlorenen Fehler-Checkpoints
> stille Neuversuche. Goals verwerfen vor einem Dispatch die alte Vorbereitung
> und benötigen frische Daten, Verification und Freigabe innerhalb des bisherigen
> Versuchslimits. Ungeklärte Dispatches verlangen ausdrücklich bezeichnete
> idempotente Recovery; spätere neue Planungsrunden erfassen frische Daten.
> Fristen, Abbruch, Queuekapazität und exklusive Reservierungen bleiben wirksam.
> 14 Prozess-Tests prüfen Konkurrenz, Kontrollrevisionen, Rollback, verlorene
> Checkpoints, erzeugte Goal-Schritte, Budgets, Fristen, CLI und additive Migration.
> Die Reason-Manager-Antwortübernahme und das gemeinsame Goal-Plan-Testsetup
> wurden nach Zuständigkeiten getrennt. Live-IPC, harte I/O-Unterbrechung,
> Controller-Fencing, allgemeine versionierte Neuplanung und die vollständige
> Langzeitabnahme bleiben offen.

> Achtzehnter Slice gemäß v3 §9.2/§9.3 und §6.5 MEM-03 sowie v2 RSN-04:
> Abgeschlossene Reasoning-V2-Versuche erhalten strukturierte Core-Belege im
> vorhandenen Artifact-Netz. Sie binden Request, exakte Dispatch-Version,
> Capability, Methode und Budget an Antwortstatus oder typisierte Fehlerklasse.
> Antwort beziehungsweise Fehlerbeleg und abschließender Zustand committen
> atomar; direkte Kontextrelationen erhalten die ursprünglichen Bezüge.
> Cache-Replay erzeugt keine weiteren Messungen. Eine begrenzte, indexierte
> Betreiberhistorie prüft gespeicherte Versionen, Zustandsbezüge und aktuelle
> Grants, ohne Kontextpayloads oder rohe Adapterfehler auszugeben. Host-Latenz,
> gemeldeter Adapterverbrauch und ungeprüfte Kandidatenqualität bleiben
> ausdrücklich getrennt. Neun Core- und sechs Prozess-Tests decken Antworten,
> Fehlschläge, explizite Wiederholung, Rollback, Paging, Neustart, Aufgabenpause
> und verlorene Queue-Checkpoints ab. Automatischer Fallback, allgemeine
> Neuplanung, Qualitäts-/Kostenoptimierung und die Phasenabnahmen bleiben offen.

> Neunzehnter Slice gemäß v3 §9.2/§9.3, §7.2 PAR-05/PAR-07/PAR-10 und v2
> RSN-06 teilweise: Eine ausdrückliche Recovery-Policy erlaubt nach belegtem
> Timeout oder Nichterreichbarkeit höchstens sieben geordnete Alternativen.
> Core-Definition und Schrittbelege binden die ursprüngliche Anfrage,
> Vorgänger, Fehler und dauerhafte Fälligkeit. Neue Requests ändern keine
> Kontextauswahl oder Action-Rechte; gleiche abgeschlossene Fehlversuche
> werden nicht erneut gestartet. Quarantänisierte Sessions können neue
> Requests lokal ablehnen, ohne Backend-Arbeit oder Binärabruf zu behaupten.
> Die ursprüngliche Reasoner-Auswahl bleibt erhalten. Joins, erzeugte Pläne
> und Goal-Verification prüfen den tatsächlich belegten Produzenten und die
> genaue Goal-Runde. Gesamtbudgets berücksichtigen alle Alternativen pro Runde;
> Pause, Abbruch und Fristen bleiben wirksam. CLI und Monitor zeigen eine
> begrenzte Recovery-Ansicht. Sechs Core- und dreizehn Prozess-Tests prüfen
> positive/negative Fälle, konkurrierende Schreiber, verlorene Checkpoints,
> Fälligkeiten, tatsächliche Produzenten und zwei manuell freigegebene
> Gerätewirkungen über aufeinanderfolgende Goal-Runden. Automatische
> Wiederverbindung, gelernte Auswahl, Modellvergleich, semantischer Fallback,
> allgemeine Neuplanung und die vollständigen Phasenabnahmen bleiben offen.

> Zwanzigster Slice gemäß v3 §7.2 PAR-02/PAR-04/PAR-08/PAR-09/PAR-10/PAR-11,
> §10.1 SEC-05 und v2 OPS-01 teilweise: Eine optionale Adapterpolicy verbindet
> ausgefallene reine Reason-Installationen neu. Zwei begrenzte Handshake-Worker
> greifen weder auf den Store zu noch führen sie Modell- oder Geräteaufträge
> aus. Der Core beansprucht jeden Versuch vor Prozessstart und übernimmt erst
> die passende, gespeicherte Session. Revisionierte Belege erhalten Fälligkeit,
> Generation und Fehlerbudget. Aktuelle Grants, exakte Metadaten, alte Endpoint-
> Nutzer, lokale Node-Besitzsperre und benannte Kapazitäten begrenzen die
> Übernahme. Task-Recovery und Connection-Recovery bleiben getrennt; gescheiterte
> Tasks werden nicht pauschal neu gestartet. Initialer Start und Reconnect
> teilen Command-Builder und Umgebungsfreigaben; Shutdown und Drop schließen
> alte Nutzer vor der Besitzfreigabe. Der Store besitzt nun einen gemeinsamen,
> nach Subject und Schema geprüften CAS-Schreibpfad; Reservierungslogik bleibt
> im zuständigen Dienst. Sechs Core- und dreizehn Prozess-Tests prüfen echte
> Neustarts, Rollback, konkurrierende Claims, begrenzte Versuche, frische
> wartende Arbeit, Isolation, Ressourcen, Umgebungswerte, Metadatenabweichung,
> RPC-Timeouts und Abbruch. Wiederverbindung anderer Rollen, Start mit zunächst
> unerreichbaren Installationen, Fähigkeitszertifizierung, Controller-Fencing
> und die vollständigen Phasenabnahmen bleiben offen.

> Einundzwanzigster Slice gemäß v3 §6.4, §6.5 MEM-03/MEM-04/MEM-05/MEM-08/
> MEM-09 und v2 MEM-06 teilweise: Authentifizierte Betreiber können einzelne
> Erinnerungs-Versionen als aktiv, strittig, zurückgezogen oder ersetzt bewerten.
> Gründe, Belege, optionale UTC-Gültigkeitsgrenzen und erwartete Revisionen
> bleiben als `arcs.memory_assessment.v1` im gemeinsamen Artifact-Netz erhalten.
> Eine optionale Abrufpolicy prüft die Bewertungen an der festen Commit-Grenze
> der Suche. `arcs.context_selection.v2` belegt Zeitpunkt, genaue Bewertungs-
> Versionen und auch Ausschlussentscheidungen. Neue Aufträge berücksichtigen
> Widerrufe; laufende Requests behalten ihre gespeicherte Identität. Quell-Trust,
> Feldgrenzen, Verification und Approval werden dadurch nicht geändert.
> Fachliche Bewertungslogik liegt im Core, der Store liefert einen allgemeinen
> begrenzten historischen Head-Abruf; die Host-CLI setzt und inspiziert Belege.
> Sieben neue Bewertungs-, fünf Kontext- und fünf Prozess-Tests prüfen unter
> anderem Revisionen, Rollback, simulierte Monate, Scopegrenzen, tatsächliche
> Adaptereingaben und Wiederaufnahme. Eine ausführbare Demo verwendet die
> vorhandenen Input- und Solverprozesse. Automatische Widerspruchserkennung,
> Konsolidierung, kalibrierte Unsicherheit, Widerruf laufender Aktionen und
> allgemeine Neuplanung bleiben offen; die Phasen sind damit nicht abgenommen.

> Zweiundzwanzigster Slice gemäß v3 §12 RSN-01/LNG-01, §13 TEST-04/TEST-05
> und v2 RSN-01/LNG-04/LNG-08: Ein ausführbarer Anwendungsnachweis verbindet
> echte externe Modellinferenz, begrenzten Gedächtnisabruf, zwei einzelne
> Testfreigaben, einen simulierten Abstand von 90 Tagen und vier Host-Neustarts.
> Ein vollständiger Lauf mit Qwen2.5 14,8B besteht mit sechs Modellaufrufen,
> genau zwei Simulatorwirkungen und unverändertem abgeschlossenen Replay.
> Fünf vorangegangene Modellfehlversuche bleiben dokumentiert. Die Vorabprüfung
> begrenzt die Testfreigaben auf das bekannte Simulatorprogramm. Inferenz bleibt
> außerhalb des Core. Der externe Adapter bindet ausdrücklich konfigurierte
> Referenzmetadaten aus dem gelieferten Kontext; fachliche Aussagen bleiben
> Modellantworten. Fünf Prozess-Tests sichern den Runner mit deterministischen
> externen Reasonern ab. v2 RSN-01 ist damit abgenommen; die weiteren genannten
> Anforderungen bleiben teilweise umgesetzt. Allgemeine Modellqualität,
> unbekannte Aufgaben ohne Routen, langfristiger realer Betrieb und autonome
> Drohnensuche bleiben eigene Abnahmen.

> Dreiundzwanzigster Slice gemäß v3 §4.1 MGR-04/MGR-05, §5.2 HET-03/HET-04,
> §7.2 PAR-02/PAR-08/PAR-10/PAR-11 und §8.2 LNG-04/LNG-05: Der bestehende
> Request-Service trennt persistente Vorbereitung, externen Aufruf und validierte
> Ergebnisübernahme. Optionale begrenzte Request-Worker erlauben unabhängige
> Goals und Quellen desselben Goals ohne blockierendes Warten. Hostweite
> Session-Zulassung berücksichtigt Reasoning und Request gemeinsam. Eine
> additive Task-/Rundenbindung begrenzt Bereinigung und schützt Pause, Abbruch,
> Fristen und Wiederaufnahme. Drei Core-, ein Worker- und dreizehn Prozess-Tests prüfen Grenzen,
> veraltete Dispatches, Grants, langsame/fehlerhafte Peers, Sensorfortschritt,
> Replay, zwei Runtimes und manuell freigegebene Simulatorwirkung. Gemeinsame
> Session-Verwaltung, Quellenbeschaffung und Worker-Lebenszyklus sind nach
> Zuständigkeit getrennt. Synchrone Act-/Output-/Polling-Ports, Request-Quoten,
> Reconnect von Mehrrolleninstallationen, allgemeine Neuplanung und vollständige
> Parallelitätsabnahme bleiben offen.

> Vierundzwanzigster Slice gemäß v3 §5.2 HET-06, §5.3, §7.2 PAR-09/PAR-10/
> PAR-11, §8.2 LNG-02/LNG-05/LNG-06/LNG-07 und v2 PLN-04/PLN-05 teilweise:
> Eine optionale Policy erlaubt nach blockierten Schritten höchstens sieben
> zusätzliche Planungen bei derselben Originalaufgabe. Versionierte
> `arcs.plan.v2`-Artefakte und begrenzte Rückblicke erhalten Vorgänger, Ergebnisse
> und genaue Produzenten. Offene alte Zweige werden dauerhaft gestoppt, bevor
> neue Vorschläge entstehen. Budgets gelten kumulativ einschließlich Planungen;
> Queuegarantien und ursprüngliche Fristen bleiben bestehen. Ersatz-Goals
> verlangen exklusive Ressourcen, frische Quellen und eigene Freigaben.
> Prozess-Tests prüfen Grenzen, Datenschutz, Fallback, verlorene Checkpoints,
> Pause, Fristen und bestätigte Simulatorwirkungen. Ein natives Rules-Beispiel
> zeigt die Planfolge. Freie Werkzeugkomposition, beliebige Ereignistrigger,
> allgemeine Strategiequalität, verteiltes Fencing und die vollständigen
> Phasenabnahmen bleiben offen.

> Fünfundzwanzigster Schritt gemäß v3 §7.2 PAR-06/PAR-10/PAR-11 und
> §8.2 LNG-04/LNG-10: Eine eigenständige Reasoning-Route kann ab Aufnahme
> einer Eingabe eine begrenzte, persistente Prüfserie starten. Nur fällige
> Prüfungen belegen einen Queueplatz. ARCS führt pro Serie eine Prüfung
> gleichzeitig aus, fasst verpasste Termine zusammen und protokolliert ihre
> Anzahl. Jeder ausgeführte Termin erhält eine eigene Reasoning-Identität.
> Prüftermine und neue Eingaben wechseln sich bei der bevorzugten Aufnahme ab.
> Kalenderregeln und die Langzeitabnahme im Betrieb bleiben offen.

> Sechsundzwanzigster Schritt gemäß v3 §7.2 PAR-07/PAR-10 und §8.2
> LNG-04/LNG-05. Betreiber können eine ganze Prüfserie über die Host-API
> oder `--series-command` pausieren, fortsetzen oder endgültig abbrechen.
> Steuerungsbelege sind revisionsgebunden und wiederholbar. Eine Serienpause
> hält verfügbare Termine und die noch offene Prüfung an. Laufende Antworten
> können in der Pause gespeichert werden. Beim Abbruch committen Serienzustand,
> Auftragsabbruch und Beleg gemeinsam, bevor der Host eine späte Antwort verwirft.

> Siebenundzwanzigster Schritt gemäß v3 §7.2 PAR-07/PAR-08/PAR-11 und §8.2
> LNG-05/LNG-08/LNG-10. Der lokale Monitor kann Aufgaben-, Serien-, Plan- und
> Goal-Befehle über seine Standardeingabe empfangen. Der vorhandene Host
> verarbeitet höchstens acht Befehle vor jeder Runde und gibt ihre Belege im
> Statusstrom aus. Die Befehle verwenden dieselben Runtime-Methoden wie die
> einzelnen CLI-Kommandos. Die Laufzeitprüfung umfasst fehlerhafte Eingaben,
> dauerhaft gespeicherte Pausen und eine versionsgebundene Simulatorfreigabe.

> Achtundzwanzigster Schritt gemäß v3 §7.2 PAR-07/PAR-08 und §8.2 LNG-05/
> LNG-08. Das GTK-Kontrollpanel steuert gemeldete Goals, Pläne und Prüfserien
> über den laufenden Monitor. Eine gezielte Leseabfrage liefert für den aktuell
> wartenden und geprüften Goal-Vorschlag die genauen Aktionsparameter. Das
> Panel bindet die Entscheidung an die angezeigte Candidate-Version. Der Host
> prüft Revision, Verification und Freigabe weiterhin im vorhandenen Pfad.

> Neunundzwanzigster Schritt gemäß v3 §7.2 PAR-01/PAR-07/PAR-11 und §8.2
> LNG-05. Die Desktop-Aufgabenwarteschlange liest offene und abgeschlossene
> Aufträge aus dem vorhandenen Journal. Seiten verwenden Sequenzcursor und
> eine feste Obergrenze für den jeweiligen Durchgang. Eigenständige Aufgaben
> und Teilschritte lassen sich mit ihrer aktuellen Kontrollrevision pausieren
> und fortsetzen. Die Ansicht zeigt Fristen, Wartezustände und vererbte Pausen.

> Dreißigster Schritt gemäß v3 §7.2 PAR-07/PAR-08/PAR-10 und §8.2 LNG-04:
> Einzelne Aufgaben lassen sich aus dem Desktop, über die CLI und im laufenden
> Monitor dauerhaft abbrechen. Im bestehenden Journal bilden Endzustand,
> Kontrollrevision und wiederholbarer Beleg eine Transaktion. Der
> Plan-Hauptauftrag verwendet den Planabbruch, ein Seriendurchlauf behält
> seine Serienzuordnung und die Serie ihren Zeitplan. Eine ungeklärte
> Reservierung nach einem Dispatch bleibt über den Neustart bestehen.

> Einunddreißigster Schritt gemäß v3 §7.2 PAR-04/PAR-10 und §8.2 LNG-08/LNG-09:
> Das Kontrollpanel findet beendete Goals mit gehaltenen Reservierungen anhand
> aktueller Core-Versionen und der zugehörigen Journal-Sitzung. Der Betreiber
> prüft ausgewählte Artefaktversionen und bestätigt die geklärte Übergabe mit
> Begründung. Die vorhandene Freigabe vergleicht den vollständigen Satz
> gehaltener Revisionen und speichert die Entscheidung im Artifact-Netz.

> Zweiunddreißigster Schritt gemäß v3 §7.2 PAR-04/PAR-10 und §7.3:
> Ein Act-Adapter kann die exakte Revision einer benannten Ressource
> verlangen. Der Core bindet sie an Invocation und Ausführungsbeleg.
> Der Lüftersimulator prüft und speichert die höchste angenommene Revision
> atomar mit dem Effekt. Eine Controller-Bestätigung beim Schließen einer
> Reservierung und die Übertragung auf reale Controller stehen aus.

> Dreiunddreißigster Schritt gemäß v3 §7.2 PAR-04/PAR-08/PAR-10 und §7.3:
> Die Laufzeit schließt eine gehaltene Revision beim Controller, bevor der
> Core die Ressource freigibt. Jede Act-Capability wird vor dem Dispatch
> eingetragen. Der Freigabe-Commit verlangt Bestätigungen aller dabei
> eingetragenen Controller. Der Simulator speichert die Sperre dauerhaft.
> Ablehnung und unterbrochene Freigabe bleiben sichtbar und halten den Besitz.

> Vierunddreißigster Schritt gemäß v3 §7.2 PAR-01/PAR-02/PAR-04/PAR-08/PAR-10/PAR-11:
> Controllerdialoge im laufenden Host verwenden begrenzte Worker und
> gemeinsame Adapterzulassung. Der Core übernimmt Bestätigungen in eigenen
> kurzen Schreibschritten und prüft beim Freigabe-Commit erneut alle
> Reservierungsrevisionen. Betreiberentscheidungen bleiben im Journal
> erhalten und werden nach einem Host-Neustart weiterverarbeitet.

> Fünfunddreißigster Schritt gemäß v3 §7.2 PAR-01/PAR-02/PAR-04/PAR-07/PAR-08/PAR-10:
> Die Goal-Runtime sendet freigegebene Aktionen über begrenzte Act-Worker.
> Adapter und Controller teilen sich die exklusive Adaptersitzung. Der Core
> hält Invocation und Reservierungsrevision vor dem Aufruf fest und
> speichert den validierten Ausführungsbeleg bei der Antwort. Ein Abbruch
> kann während des Aufrufs gespeichert werden. Eine ungeklärte Reservierung
> bleibt bis zur bestätigten Übergabe gehalten.

> Sechsunddreißigster Schritt gemäß v3 §7.2 PAR-08/PAR-09/PAR-10:
> Verbuchte Act-Fehler beenden einzelne Goals auch in der Standardkonfiguration.
> Status, Fehlertext und Goal-Checkpoint werden im Journal gemeinsam
> aktualisiert. Der Wiederanlauf übernimmt einen bereits im Core gespeicherten
> Fehler. Nach einem fehlgeschlagenen Ergebnis-Commit bleibt ein unterbrochener
> Dispatch mit seiner ursprünglichen Invocation-ID wiederaufnehmbar.

> Siebenunddreißigster Schritt gemäß v3 §7.2 PAR-01/PAR-02/PAR-08/PAR-10/PAR-11:
> Der Scheduler startet Input- und Observe-Dialoge auf vorhandenen
> stdio-Transport-Threads. Er verarbeitet Antworten in späteren Takten.
> Der Core erteilt die Bestätigung erst nach validiertem Commit. Die
> Runtime begrenzt laufende Dialoge, prüft die Zulassung verspäteter
> Inputs erneut und zeigt Adapterfehler einzeln im Monitor an.

> Achtunddreißigster Schritt gemäß v3 §7.2 PAR-01/PAR-02/PAR-08/PAR-10/PAR-11:
> Alle Goal-Quellenabfragen laufen über denselben begrenzten Request-Pool.
> Ein einzelner Request-Platz lässt den Scheduler für unabhängige Aufgaben
> verfügbar. Ein bereits gespeicherter Quellenfehler wird beim Wiederanlauf
> in den Aufgabenstand übernommen. Fehlschläge beim Commit einer validierten
> Antwort lassen den Dispatch mit derselben Invocation-ID wiederaufnehmbar.

> Neununddreißigster Schritt gemäß v3 §7.2 PAR-01/PAR-02/PAR-07/PAR-08/PAR-10:
> Reasoning belegt auch beim Standardwert eins einen Worker. Die Runtime
> verarbeitet während des Modellaufrufs andere Manager und Steuerbefehle.
> Fehlerisolierung gilt für alle Limits. Ein verbuchter Modellfehler wird
> beim Wiederanlauf übernommen. Ein unterbrochener Ergebnis-Commit lässt
> denselben Dispatch wiederaufnehmbar. Neuplanung nutzt dieselben Regeln.

> Vierzigster Schritt gemäß v3 §7.2 PAR-01/PAR-02/PAR-04/PAR-08/PAR-10/PAR-11
> und §11 SEC-03. Die Runtime übernimmt bereits geprüfte und freigegebene
> Ausgaben per Monitor oder CLI in ihr Journal. Begrenzte Worker teilen sich
> die Adapterzulassung mit den übrigen Managern. Der Core speichert Dispatch
> und Quittung unter einer stabilen Identität. Nach einem Neustart setzt die
> Runtime die Zustellung fort oder übernimmt die vorhandene Quittung.

> Einundvierzigster Schritt gemäß v3 §4 Output, §7.2 PAR-10/PAR-11 und
> §10.1 SEC-03/SEC-04/SEC-09. Der Betreiber wählt ein konkretes Ergebnis
> einer fertigen Aufgabe oder eines aktuellen Planschritts. ARCS erzeugt
> einen Bericht mit belegbarer Herkunft und prüft den Zielvertrag. Die
> Freigabe bindet Vorschau und ausgewählten Adapter. Die Runtime speichert
> die Entscheidung vor der Übergabe und übernimmt bereits gespeicherte
> Freigaben nach einem Neustart.

> Zweiundvierzigster Schritt gemäß v3 §4 Output, §7.2 PAR-10/PAR-11 und
> §10.1 SEC-03/SEC-04/SEC-09. Betreiber können für eine Planvorlage einen
> Zieladapter und eine Ergebnisposition festlegen. Die Runtime speichert
> die Berichtsvormerkungen beim Abschluss der erfolgreichen Plangeneration
> und bereitet sie in begrenzten Gruppen vor. Der Core bindet die
> veröffentlichten Exportfelder an das konkrete Ergebnis und die genaue
> Planversion. Desktop und CLI verwenden die vorhandene Freigabe und
> Zustellwarteschlange.

> Dreiundvierzigster Schritt gemäß v3 §7.2 PAR-10/PAR-11 und §8.2 LNG-06.
> Eine Eingangsroute kann die Zahl neuer Reasoning-Request-IDs je Aufgabe
> begrenzen. Pläne teilen das Kontingent mit ihren Schritten und Revisionen.
> Die Runtime prüft und reserviert Plätze im bestehenden Journal. Sie
> reserviert die Wirkungsbewertung vor der Aktorausführung. CLI und GTK zeigen
> den Verbrauch und das Limit pro Aufgabe.

> Vierundvierzigster Schritt gemäß v3 §7.2 PAR-02/PAR-10 und §8.2
> LNG-02/LNG-05/LNG-07. Ein Ersatzplan kann die sichtbaren, freigegebenen
> Exporte aus erfolgreichen Schritten seines direkten Vorgängers als
> Eingaben verwenden. Das Planartefakt hält die genauen Versionen fest.
> Die Runtime übernimmt die Bindungen mit der neuen Generation in die
> bestehende Abhängigkeitsverwaltung und führt die neuen Schritte nach
> ihren normalen Budgets aus.

> Fünfundvierzigster Schritt gemäß v3 §4 Act, §7.2 PAR-10 und §8.2
> LNG-09. Der Workspace-Check-Adapter startet freigegebene, in seinem Profil
> benannte Softwareprüfungen. Er speichert die Startabsicht vor der Ausführung
> und bringt Status, Exitcode und Auftragsbezug als Ausführungsartefakt in
> den Core. Der Schritt stellt einen Werkzeugpfad für TEST-02 bereit.

> Sechsundvierzigster Schritt gemäß v3 §8.2 LNG-05, LNG-08, LNG-09
> und TEST-02. Eine Goal-Route kann ihren Erfolg an ein Betreiberschema
> für das konkrete Ausführungsartefakt binden. Ein externes Assessment
> muss diese Quittung ausdrücklich zitieren. Das Softwarebeispiel
> verwendet den Workspace-Check-Adapter und demonstriert einen
> fehlgeschlagenen und einen später bestandenen Test in derselben Aufgabe.

> Siebenundvierzigster Schritt gemäß v3 §4 Act, §7.2 PAR-10 und
> TEST-02. Der Workspace-Adapter kann eine freigegebene Textdatei auf
> Basis eines konkreten Git-Commits ersetzen. Git veröffentlicht das
> Ergebnis per atomarem Vergleich auf einer lokalen ARCS-Referenz.
> Das Adapterjournal erhält die Vorbereitungsdaten und die Quittung
> über Neustarts. Ein konfigurierter Test prüft den neuen Git-Stand.

> Achtundvierzigster Schritt gemäß v3 §4 Act, §7.2 PAR-10, §8.2
> LNG-09 und TEST-02. Freigegebene Prüfaufträge binden ein benanntes
> Prüfprogramm an einen vollständigen Commit der lokalen ARCS-Referenz.
> Der Workspace-Adapter führt das Programm mit den Dateien dieses
> Git-Baums aus und quittiert die geprüfte Revision im Artifact-Netz.
> Wiederaufnahme verwendet die gespeicherte Quittung. Die
> automatische Übergabe der Edit-Quittung an einen Prüfschritt
> desselben Plans bleibt offen.
>
> Neunundvierzigster Schritt gemäß v3 §5.2 HET-04, §8.2 LNG-05/LNG-09
> und TEST-02. Ein erfolgreich abgeschlossenes Goal kann seine geprüfte
> Ausführungsquittung als Eingabe für nachfolgende Schritte exportieren.
> Im Softwarebeispiel bindet ein Plan die Edit-Quittung an die
> revisionsgenaue Prüfung. Ein Neustart zwischen Korrektur und Prüfung
> erhält die Zuordnung und die separate Freigabe für die Prüfung.
>
> Fünfzigster Schritt gemäß v3 §5.2 HET-06, §8.2 LNG-05/LNG-09
> und TEST-02. Der Ersatzplaner kann die gezielt freigegebene Quittung
> einer nachweislich fehlgeschlagenen Ausführung als Kontext erhalten.
> Der Plan hält Fehlversuch und erfolgreiche Exporte getrennt.
> Ein Softwaretest demonstriert den Wechsel zu einer zweiten
> Korrektur mit eigener Prüfung und erneuten Freigaben.
>
> Einundfünfzigster Schritt gemäß v3 §4 Act, §5.2 HET-06,
> §8.2 LNG-09 und TEST-02. Eine eigens freigegebene revisionsgenaue
> Prüfung speichert begrenzte Standardausgaben bei Fehlschlägen.
> Der Ersatzplaner kann diesen Text über ein separates Exportfeld
> auswerten. Prozess- und Integrationstests prüfen die Ausgabegrenze,
> Wiederaufnahme und den Befund aus einer realen fehlgeschlagenen Assertion.
>
> Zweiundfünfzigster Schritt gemäß v3 §4 Request, §5.2 HET-04/HET-06,
> §8.2 LNG-09 und TEST-02. Der Request-Adapter kann freigegebenen
> Quelltext eines genauen Commits liefern. Eine Goal-Quelle kann
> die Request-Eingabe aus einem gebundenen Vorgängerexport ableiten.
> Im Prozess-Test entsteht die neue Korrektur aus dem gelesenen
> Quelltext und wird nach Freigabe auf einem neuen Commit geprüft.
>
> Dreiundfünfzigster Schritt gemäß v3 §4 Reason/Act, §5.2 HET-04/HET-06
> und TEST-02. Ein Ersatzplan kann eine gebundene Fehlerquittung an
> einen neuen Reparaturschritt weitergeben. Der Modelladapter erhält
> den revisionsgenauen Quelltext und die Prüfdiagnose. Die Korrektur
> und der Test des neu veröffentlichten Commits behalten ihre
> getrennten Freigaben.
>
> Vierundfünfzigster Schritt gemäß v3 §10.1 SEC-03/SEC-04/SEC-09
> und TEST-02. Betreiber können Aktionsfelder an Werte aus
> festgehaltenen Vorher-Quellen binden. Der Reparaturablauf prüft
> Pfad und Basis-Commit vor Freigabe und Act. Prozess- und Modelltests
> decken passenden und abweichenden Quellbezug ab.
>
> Fünfundfünfzigster Schritt gemäß v3 §4 Act, §10.1 SEC-06
> und TEST-02. Freigegebene Git-Prüfungen können in einem eigenen
> Bubblewrap-Namespace laufen. Die Git-Kopie ist schreibgeschützt.
> Der Sandbox-Start erzeugt einen separaten Laufbeleg. Der
> bestehende Reparaturplan verarbeitet auch diese Prüfergebnisse.
>
> Sechsundfünfzigster Schritt gemäß v3 §7.2 PAR-07/PAR-08,
> §10.1 SEC-06/SEC-07 und TEST-02. Ein delegierter cgroup-v2-Bereich
> ermöglicht CPU-, Speicher- und Taskgrenzen für den gesamten
> Prozessbaum einer isolierten Git-Prüfung. Der Adapter unterscheidet
> Grenzverletzungen von fachlichen Testfehlern und dokumentiert
> die Kernelzähler im Ausführungsartefakt.
>
> Siebenundfünfzigster Schritt gemäß v3 §7.2 PAR-07/PAR-10,
> §10.1 SEC-06/SEC-07 und TEST-02. Der Workspace-Adapter hält
> die Identität seiner Prüfgruppen dauerhaft fest. Beim Neustart
> räumt er eigene Gruppen auf. Ein Aufruf mit derselben
> Ausführungsidentität liefert `interrupted`, wenn die Prüfung
> ohne gespeichertes Ergebnis abgebrochen wurde.
>
> Achtundfünfzigster Schritt gemäß v3 §8.2 LNG-03/LNG-04/LNG-07.
> Ein gespeicherter Aktivierungsbeleg kann eine autorisierte
> Ereignisquelle mit einem relativen Termin verbinden. Auch
> zwischen Zielrunden kann ein Signal früher wecken, während
> der Termin eine erneute Prüfung bei ausbleibendem Signal auslöst.
>
> Neunundfünfzigster Schritt gemäß v3 §6.5 MEM-03 sowie §8.2
> LNG-02/LNG-05/LNG-09. Bewertete Zielrunden haben einen eigenen
> Beleg im bestehenden Artifact-Netz. Er verbindet Aufgabe,
> vorherige Runde, Bewertung und gegebenenfalls Wirkungsbeleg.
> Der gespeicherte Zielstand verweist auf das Ende dieser Kette.
> Der begrenzte Gedächtnisabruf kann daraus früheren Fortschritt
> in eine neue Zielrunde übernehmen.
>
> Sechzigster Schritt gemäß v3 §8.2 LNG-07/LNG-09 und TEST-01.
> Eine Nachher-Abfrage kann die bestätigte Zustandsrevision aus
> der eigenen Ausführungsquittung in den Request übernehmen.
> Der Beleg des Requests verknüpft beide Versionen. Die vorhandene
> externe Drohnenkamera nimmt damit zur bestätigten Revision auf.
>
> Einundsechzigster Schritt gemäß v3 TEST-01. Die externe
> Farbsuche startet mit wiederholbaren Szenarien im Simulator.
> Ein begrenzter Suchlauf bestätigt Bildfund und Bewegung anhand der
> jeweiligen Aufnahme. Bei fehlendem Ziel endet er am Rundenbudget.
> Der Quellstand der Suchanwendung liegt in `drone-sim`.
>
> Zweiundsechzigster Schritt gemäß v3 §8.2 LNG-09, §10.1 SEC-03
> und TEST-01. Das Drohnenprojekt lädt die Aufnahme des bestätigten
> Funds anhand ihrer gespeicherten Version und prüft den Farbtreffer
> erneut. Die Abschlussbewertung kann über den vorhandenen
> Output-Vertrag gesondert zur Zustellung freigegeben werden.
>
> Dreiundsechzigster Schritt gemäß v3 §9.2 und §9.3. Bei gleicher
> Relevanz können gespeicherte technische Reasoning-Versuche die
> Auswahl freier Reasoner beeinflussen. Der Core speichert die
> Stichproben am Auswahlbeleg. Die Betreiberpolicy legt Gewichte,
> Mindeststichprobe und Zeitskala fest.
>
> Vierundsechzigster Schritt gemäß v3 §7.2 PAR-04/PAR-11 und §9.2.
> Die erstmalige Planerauswahl zählt autorisierte Binärdaten und prüft
> die gemeinsame Poolanforderung von Reasoner und Datenquellen. Eine
> inkompatible Option kann die Bindung eines geeigneten Planers nicht
> mehr verhindern. Der vorhandene Dispatch prüft erneut vor dem Aufruf.
>
> Fünfundsechzigster Schritt gemäß v3 §8.1, §8.2 LNG-05, LNG-07 und LNG-08
> sowie §9.2. Zielrouten können die Planerauswahl an eine Runde binden.
> ARCS nutzt beim Eintritt in die Planung die aktuelle Verfügbarkeit
> und verknüpft Freigabe und Recovery mit dem Beleg dieser Runde.
>
> Sechsundsechzigster Schritt gemäß v3 §9.2 und §7.2 PAR-04.
> Der Planer kann pro Revision einen Beleg über die Verfügbarkeit
> der anfänglichen Reasoner-Optionen seiner Vorlagen erhalten.
> Nach einem Neustart verwendet dieselbe Revision denselben Beleg.
>
> Siebenundsechzigster Schritt gemäß v3 §9.1 und §9.2, CAP-03.
> Empfangende Vorlagen können Ergebnisschemas für ihre Abhängigkeiten
> vorgeben. Der Planer sieht die Exportverträge und der Host prüft
> sie vor Aufnahme des Plans sowie beim Laden einer gespeicherten Revision.
>
> Achtundsechzigster Schritt gemäß v3 §5.2 MGR-07 und §9.2, CAP-03.
> Die bestehende Pipeline findet passende Transform-Pfade zwischen
> registrierten Schemas. Sie begrenzt die Suche auf Betreiber-Grants
> und bindet die ausgewählten Schritte an ihre gespeicherte Definition.
>
> Neunundsechzigster Schritt gemäß v3 §5.2 MGR-07 und §9.1, CAP-03.
> Pipelines können ausgewählte Felder in ein geprüftes Eingabeartefakt
> projizieren. Transform verwendet die Projektionsversion und ihr Schema.
> Die Projektion bleibt über Provenienz mit der Quelle verbunden.
>
> Siebzigster Schritt gemäß v3 §5.2 MGR-04/MGR-07 und §7.2 PAR-04/PAR-11.
> Der Host kann Transform parallel über begrenzte Worker ausführen.
> Adapter-Sitzungen und benannte Kapazitäten werden mit den anderen
> Aufrufen geteilt. Der Core prüft und speichert jedes Ergebnis.
>
> Einundsiebzigster Schritt gemäß v3 §5.2 MGR-07, §7.2 PAR-04 und §9.2, CAP-03.
> Planvorlagen können pro Quellschema freigegebene Transform-Verträge nutzen.
> Der Nachfolger wartet auf die Projektion, den Transform-Worker und das
> schema-geprüfte Ergebnis.
>
**Leitbild:** ARCS ist ein universeller Agent. Es soll sich einfach bedienen lassen, beliebige kompatible Systeme verbinden, eigenständig Probleme lösen und langfristige Aufgaben über Monate verfolgen.

## 1. Verbindliche Architekturregeln

1. **Ein Core:** Keine zweite übergeordnete Agentensteuerung.
2. **Sechs spezialisierte Manager:** Input, Observe, Request, Reason, Act und Output bilden die Schnittstellen zur Außenwelt.
3. **Transform bleibt die siebte Adapter-Rolle:** Datenverarbeitung ist überall nutzbar, aber kein verpflichtender Durchgangspunkt.
4. **Ein gemeinsames Artifact-Netz:** Artefakte sind zugleich Arbeitsdaten, historische Belege und Grundlage des Gedächtnisses.
5. **Externe Intelligenz:** LLMs, Jev, Solver und andere Reasoner bleiben unabhängig vom Core.
6. **Einheitliche Sicherheitsgrenzen:** Verification, Approval, Grants und Provenienz dürfen von keinem Manager umgangen werden.
7. **Universelle Komponenten:** Fachwissen über Drohnen, Datenbanken oder bestimmte Modelle gehört nicht in den allgemeinen Core.

## 2. Bestehende Grundlage

**Bereits vorhanden oder teilweise implementiert:**

- Versionierte Artefakte, Beziehungen, Provenienz, Trust und persistenter SQLite-Store.
- Artifact-Netz mit einem bereits implementierten bekannten Fast Path.
- Adapter-Protokoll und SDK für alle sieben Rollen.
- Mehrere unabhängige Adapterinstallationen und Capability-Katalog.
- Allgemeiner Host, Grants, Verification und Approval.
- Transform-Pipelines, Reasoning V2 und kontrollierte Aktionen.
- Persistente Aufgabenqueue mit Neustart-Wiederaufnahme.
- Begrenzte Goal-Runtime mit Planung, Ausführung und Wirkungsprüfung.
- Erste Desktop-GUI mit Texteingabe und Modellkonfiguration.

**Noch nicht ausreichend nachgewiesen:** Allgemeine Problemlösung mit echten Modellen, autonomes Fähigkeitsrouting, langfristige Aufgabenbearbeitung, wiederverwendbare Erinnerungen und autonome Drohnensuche.

---

## Phase 1 – Architektur konsolidieren

**Priorität: P0**

Ziel: Die Sternarchitektur verbindlich abbilden, ohne die funktionierende Infrastruktur neu zu schreiben.

- [ ] **ARC-01:** Architekturvertrag für Core, sechs Manager, Transform und externe Adapter festlegen.
- [ ] **ARC-02:** Vorhandene Host- und Core-Funktionen den sechs Manager-Zuständigkeiten zuordnen.
- [ ] **ARC-03:** Gemeinsame Registry, Kommunikation, Berechtigungen und Adapter-Identitäten beibehalten.
- [ ] **ARC-04:** Klare Schnittstellen der Manager definieren; zunächst als Module, nicht als sechs neue Prozesse.
- [ ] **ARC-05:** Einen Adapter mit mehreren Rollen über verschiedene Manager nutzen und testen.
- [ ] **ARC-06:** Bisherige Tests nach der Umstrukturierung vollständig erneut ausführen.

**Abnahme:** Unterschiedliche Geräte können über dieselben allgemeinen Verträge angeschlossen werden. Der Core besitzt keine gerätespezifischen Sonderwege, und vorhandene Funktionen bleiben erhalten.

## Phase PAR – Allgemeine Parallelitätsarchitektur

**Priorität: P0, verbindliche Grundlage ab Phase 1.** Aufgaben-, Schritt- und
Komponentenparallelität werden gemeinsam mit Core und Managern entwickelt.

- [ ] **PAR-01:** Unabhängige Aufgaben ohne gegenseitige Blockierung ausführen.
- [ ] **PAR-02:** Parallele Planschritte mit Abhängigkeiten, Join-Regeln und
  definiertem Verhalten bei Teilausfällen darstellen.
- [ ] **PAR-03:** Reasoner und Adapter unabhängig und begrenzt parallel aufrufen.
- [ ] **PAR-04:** Gemeinsame Ressourcen einschließlich Geräten, GPU und RAM
  anhand stabiler Identitäten reservieren; Kapazitäten und Konflikte prüfen.
- [ ] **PAR-05:** Aufgaben- und Artefaktänderungen atomar behandeln; verspätete
  Worker-Ergebnisse anhand der aktuellen Ausführungsidentität zurückweisen.
- [ ] **PAR-06:** Fairness, Prioritäten, Timeouts, Abbruch und Backpressure
  durchsetzen, ohne eine unbegrenzte Zahl wartender Threads zu erzeugen.
- [ ] **PAR-07:** Komponentenfehler isolieren und Wiederaufnahme mit denselben
  Invocation-IDs sowie deduplizierenden Adaptern durchführen.
- [ ] **PAR-08:** Parallelität, konkurrierende Ressourcen und Recovery unter Last
  prüfen. Ein erfolgreicher Normalfall allein genügt nicht.

**Abnahme:** Drei unabhängige Aufgaben schreiten fort. Ein hängender Reasoner
blockiert die übrigen Aufgaben und Sensoren nicht. Exklusive Geräte erhalten
keine konkurrierenden Aufträge. Ein Neustart nach einer verlorenen Antwort
führt bei deduplizierenden Adaptern zu keiner zweiten externen Wirkung.

Ein gemeinsamer Core darf mehrere Worker koordinieren. Er hält während
externer I/O keine globale Sperre und keine offene Schreibtransaktion.
Arbeitskontexte sind an konkrete Artefaktversionen gebunden. Ressourcen werden
nicht allein wegen einer abgelaufenen Reservierung als sicher frei betrachtet.

## Phase 2 – Artifact-Netz zum Gedächtnis erweitern

**Priorität: P0**

Ziel: Der Artifact Store wird nicht durch ein zweites Memory-System ersetzt, sondern selbst zum langfristig nutzbaren Gedächtnis ausgebaut.

- [ ] **MEM-01 – Erinnerungstypen:** Ereignisse, Fakten, Erfahrungen, Ziele, Entscheidungen und wiederverwendbare Verfahren als Artefakte oder aus ihnen abgeleitete Strukturen repräsentieren.
- [ ] **MEM-02 – Assoziationen:** Beziehungen wie `derived_from`, `supports`, `contradicts`, `supersedes`, `part_of_goal` und `solved_by` ergänzen.
- [ ] **MEM-03 – Retrieval:** Artefakte anhand von IDs, Beziehungen, Bedeutung, Zeitpunkt und aktueller Aufgabe wiederfinden.
- [ ] **MEM-04 – Kontextauswahl:** Relevante Artefakte automatisch zu einem begrenzten Reasoning-Kontext zusammenstellen.
- [ ] **MEM-05 – Konsolidierung:** Aus mehreren Erfahrungen zusammenfassende Erkenntnisse erzeugen, aber immer mit Referenzen auf die ursprünglichen Belege.
- [ ] **MEM-06 – Aktualisierung:** Widersprüche und veraltete Informationen erkennen; historische Originalartefakte nicht stillschweigend überschreiben.
- [ ] **MEM-07 – Effizienz:** Indexierung, Caching und optional semantische Vektorsuche integrieren, ohne einen zweiten unabhängigen Wahrheitsbestand zu schaffen.
- [ ] **MEM-08 – Gedächtnistests:** Abrufqualität, fehlende relevante Erinnerungen, falsche Treffer, Tokenverbrauch und Antwortqualität messen.

**Abnahme:** ARCS kann eine frühere Aufgabe nach einem Neustart fortsetzen und die entscheidenden Erfahrungen finden, ohne die vollständige Historie erneut an ein LLM zu senden.

## Phase 3 – Echte Reasoner und intelligentes Routing

**Priorität: P0**

Ziel: ARCS verwendet unterschiedliche Lösungsverfahren zweckmäßig.

- [x] **RSN-01:** Ein echtes lokales LLM mit ungefähr 14B Parametern über Reasoning V2 testen. Nachweis: [14,8B-Inferenzbericht](ZWISCHENBERICHT_2026-11-19_INFERENCE.md).
- [ ] **RSN-02:** Jev beziehungsweise das gewählte entscheidungsorientierte Modell als unabhängigen Adapter integrieren.
- [ ] **RSN-03:** Deterministische Solver und Regeln als gleichwertige Alternativen erhalten.
- [ ] **RSN-04:** Einheitliche strukturierte Ergebnisse, Fehlermeldungen und Herkunftsinformationen verwenden.
- [ ] **RSN-05:** Reasoning-Verfahren anhand von Aufgabenart, Eignung, Latenz, Ressourcen und Kosten auswählen.
- [ ] **RSN-06:** Fehler-Fallbacks und optionalen Modellvergleich ergänzen.
- [ ] **RSN-07:** Die Modelle an identischen Aufgaben hinsichtlich Erfolg, Zuverlässigkeit und Ressourcenbedarf vergleichen.

**Abnahme:** ARCS kann zwischen LLM, Entscheidungsmodell und Solver unterscheiden, statt für jede Aufgabe einen fest vorgegebenen Reasoner zu benötigen.

## Phase 4 – Langfristige Aufgaben und persistente Autonomie

**Priorität: P0**

Ziel: Aufgaben können Stunden, Tage oder Monate bestehen bleiben, ohne dass ein LLM ununterbrochen aktiv sein muss.

- [ ] **LNG-01 – Dauerhafte Ziele:** Projekte, Teilziele, Fristen, Abhängigkeiten, Fortschritt und offene Entscheidungen persistent speichern.
- [ ] **LNG-02 – Scheduler:** Zeitbasierte, ereignisbasierte und bedingte Auslöser ergänzen.
- [ ] **LNG-03 – Zustandsübergänge:** `active`, `waiting`, `blocked`, `awaiting_approval`, `completed`, `failed` und `cancelled` zuverlässig behandeln.
- [ ] **LNG-04 – Wiederaufnahme:** Nach Absturz, Neustart oder tagelanger Unterbrechung korrekt weiterarbeiten.
- [ ] **LNG-05 – Ressourcen:** Aufgabenbudgets, Prioritäten, maximale Laufzeiten und Nutzungsgrenzen definieren.
- [ ] **LNG-06 – Nebenläufigkeit:** Mehrere unabhängige Aufgaben bearbeiten, ohne Konflikte oder doppelte externe Wirkungen zu erzeugen.
- [ ] **LNG-07 – Überprüfung:** Periodisch oder ereignisabhängig prüfen, ob das langfristige Ziel noch relevant ist und welche Voraussetzungen fehlen.
- [ ] **LNG-08 – Langzeittest:** Mit simulierter Zeit und anschließend realen Unterbrechungen prüfen, ob eine Aufgabe über längere Zeit korrekt fortgeführt wird.

**Abnahme:** Eine Aufgabe wird gespeichert, unterbrochen, später durch ein Ereignis wieder aktiviert, mit ihrem bisherigen Kontext fortgesetzt und anschließend nachvollziehbar abgeschlossen.

Der Scheduler gehört zur bestehenden Core-/Runtime-Infrastruktur. Er ist keine neue übergeordnete Kontrollinstanz.

## Phase 5 – Dynamisches Fähigkeitsmanagement und allgemeine Planung

**Priorität: P0**

Ziel: ARCS kann eigenständig geeignete Werkzeuge finden und zu einem ausführbaren Ablauf kombinieren.

- [ ] **CAP-01:** Capability-Beschreibungen um Zweck, Voraussetzungen, Wirkungen, Nebenwirkungen, Grenzen und Datenverträge erweitern.
- [ ] **CAP-02:** Geeignete Fähigkeiten aus dem gemeinsamen Katalog anhand einer Zielanforderung finden.
- [ ] **CAP-03:** Kompatibilität zwischen Capabilities prüfen und nötige Transform-Schritte auswählen.
- [ ] **PLN-01:** Ziele in überprüfbare Teilziele und Abhängigkeiten zerlegen.
- [ ] **PLN-02:** Mehrstufige Pläne mit Bedingungen, Alternativen und optional parallelen Schritten darstellen.
- [ ] **PLN-03:** Vor jeder Ausführung Berechtigungen, Voraussetzungen und Ressourcen prüfen.
- [ ] **PLN-04:** Nach fehlgeschlagenen Aktionen, unerwarteten Beobachtungen oder neuen Informationen umplanen.
- [ ] **PLN-05:** Pläne versionieren und mit den entsprechenden Artefakten und Ergebnissen verknüpfen.

**Abnahme:** ARCS erhält eine bislang nicht speziell vorkonfigurierte Aufgabe, sucht die verfügbaren Fähigkeiten und erstellt daraus einen überprüfbaren Arbeitsablauf.

## Phase 6 – Sichere Werkzeugnutzung und eigener Fähigkeitsausbau

**Priorität: P1**

Ziel: ARCS soll nicht an fehlenden Werkzeugen scheitern, wenn diese kontrolliert entwickelt oder ergänzt werden können.

- [ ] **TOL-01:** Allgemeine Adapter für Dateisystem, Prozesse, Entwicklungswerkzeuge und ausgewählte externe APIs integrieren.
- [ ] **TOL-02:** Isolierte Umgebungen für fremden oder generierten Code bereitstellen.
- [ ] **TOL-03:** Workflow für Werkzeugentwicklung: Anforderungen → Entwurf → Implementierung → Tests → Prüfung.
- [ ] **TOL-04:** Neu erzeugte Adapter automatisch gegen ihre Capability-Verträge testen.
- [ ] **TOL-05:** Installation, Versionierung, Freigabe und Rollback von Fähigkeiten unterstützen.
- [ ] **TOL-06:** Wiederverwendbare erfolgreiche Verfahren im Artifact-Netz speichern und bei ähnlichen Aufgaben abrufen.

**Abnahme:** ARCS kann für eine Aufgabe ein fehlendes kleines Werkzeug entwickeln, sicher testen, nach entsprechender Freigabe verwenden und bei späteren Aufgaben wiederfinden.

## Phase 7 – Robotik und Drohnen

**Priorität: P1, als fortlaufender Integrationstest bereits früher beginnen**

Ziel: Die allgemeine Agentenarchitektur funktioniert auch mit dynamischen physischen oder simulierten Systemen.

- [ ] **DRN-01:** Drohnenadapter für Beobachtung, Abfragen und Aktionen an die sechs Manager anschließen.
- [ ] **DRN-02:** Kamera- und Positionsdaten mit korrekten Zeit-, Koordinaten- und Qualitätsinformationen verknüpfen.
- [ ] **DRN-03:** Einen externen Wahrnehmungs-Reasoner für Zielobjekte nutzen.
- [ ] **DRN-04:** Eine unbekannte Umgebung schrittweise erkunden und ein Zielobjekt suchen.
- [ ] **DRN-05:** Bewegungsplanung, Hindernisvermeidung und erneute Beobachtungen kombinieren.
- [ ] **DRN-06:** Sichere lokale Flugsteuerung, Abbruchbedingungen und Kommunikationsausfälle berücksichtigen.
- [ ] **DRN-07:** Einen Fund ausschließlich mit nachvollziehbarer Evidenz als erreicht melden.

**Abnahme:** ARCS findet in einer unbekannten simulierten Umgebung ein Zielobjekt, ohne dass ihm eine feste Bewegungsfolge vorgegeben wurde.

Die zeitkritische Flugstabilisierung bleibt Aufgabe eines spezialisierten Controllers, etwa PX4, nicht eines LLM.

## Phase 8 – Benutzerfreundlichkeit, Sicherheit und Betrieb

**Priorität: Querschnittsaufgabe, von Anfang an mitentwickeln**

- [ ] **UX-01:** Ziele natürlich formulieren können, ohne Routen und JSON-Dateien bearbeiten zu müssen.
- [ ] **UX-02:** Aufgaben, Teilziele, Fortschritt und Ergebnisse verständlich darstellen.
- [ ] **UX-03:** Manuelle Freigaben direkt in der GUI ermöglichen.
- [ ] **UX-04:** Adapter und Modelle über einfache Einrichtungsdialoge verbinden.
- [ ] **SEC-01:** API-Schlüssel nur an berechtigte Adapterprozesse weitergeben.
- [ ] **SEC-02:** Berechtigungen, Rate Limits, Isolation und Ressourcenbudgets durchsetzen.
- [ ] **OPS-01:** Verbindungsüberwachung, Fehlerwiederherstellung und Protokollierung verbessern.
- [ ] **OPS-02:** Benchmarks und Regressionstests mit mehreren unterschiedlichen Aufgaben automatisieren.

**Abnahme:** Ein Benutzer kann ARCS über eine einfache Oberfläche ein Ziel geben, benötigte Verbindungen herstellen und die Bearbeitung nachvollziehen, ohne die internen Manager verstehen zu müssen.

---

## Phase EXT-0 – Vertragsabgleich der Zielbilderweiterung

**Priorität: P0. Voraussetzung für alle EXT-Phasen.**

Ziel: Bestehende Goal-Runtime, Task-Queues, Journale, Ressourcenreservierungen,
Memory-Scopes, Admission-Regeln und Adapterverträge werden geprüft und
**erweitert**, nicht dupliziert. Ergebnis ist eine dokumentierte Zuordnungstabelle
und keine Parallelarchitektur.

- [ ] **EXT-00:** Abgleich von `CTX-01`, `CTX-05`, `CTX-08`, `PRIV-01`, `PRIV-02` und `OPS-03` gegen die bestehenden Verträge in `apps/adapter-host/src/task_runtime/**`, `crates/arcs-core/src/runtime/{invocation,reservations}/**`, `crates/arcs-core/src/reasoning/context.rs` und `crates/arcs-core/src/core/{artifact,schema}/**`.
- [ ] **EXT-00:** Je Erweiterungs-ID eine konkrete Erweiterungsstelle benennen; änderungen ohne Zuordnung sind zurückzuweisen.
- [ ] **EXT-00:** ID-Namensraum der Erweiterung (`CTX-*`, `PRIV-*`, `FORM-*`, `OPS-*`, `BIZ-*`) gegen die bestehenden v2- und v3-Kürzel dokumentieren; `OPS-*` der Erweiterung ist **nicht** `OPS-01`/`OPS-02` aus Phase 8 und **nicht** `OPS-01` aus v3 §12.

**Abnahme:** Für jede neue Anforderung ist eine Erweiterungsstelle benannt.
Es existiert kein zweiter Core, kein zweiter Store und keine Parallelführung
bestehender Verträge.

---

## Phase EXT-CTX – Unified Multi-Context Runtime

**Priorität: P0 (CTX-01 bis CTX-05), P1 (CTX-06, CTX-07).**
Ergänzt v3 §5, §6, §7 und §8. Siehe
[Zielbilderweiterung §18](ZIELBILD_ERWEITERUNG_2026-11-27.md).

- [ ] **EXT-CTX-01 (CTX-01):** Leichten persistenten Arbeitskontext einführen, der Ziel, Task, Planrevision, Artifact-Versionen, Arbeitszustand, Authority-Scope, zulässige Memory-Sicht, Priorität, Budget und laufende Operationen bündelt.
- [ ] **EXT-CTX-02 (CTX-02):** Gemeinsame Nutzung von Core-Diensten, Adapterregistrierung, Verifier, Artifact-Store und Reasonern nachweisen; Session- und KV-Isolation je Tätigkeit regeln.
- [ ] **EXT-CTX-03 (CTX-03):** Sichere Kontextwechsel mit explizitem Checkpoint einführen; wartende Aufgaben ohne aktive Modellinstanz fortsetzen; laufende externe Operationen beim ursprünglichen Task halten.
- [ ] **EXT-CTX-04 (CTX-04):** Fairness, Backpressure, Budgetabrechnung und Prioritätsklasse für interaktive Anfragen im Scheduler ergänzen; Preemption nur an sicheren Grenzen.
- [ ] **EXT-CTX-05 (CTX-05):** Resultate, Events, Toolantworten und Freigaben eindeutig an Task, Kontext, Planrevision und Operation binden; keinen globalen veränderlichen „aktuellen Kontext“ einführen.
- [ ] **EXT-CTX-06 (CTX-06):** Agent Spawning als optionale, begrenzte Fähigkeit für Isolation und externe Delegation dokumentieren – nicht als Normalmechanismus.
- [ ] **EXT-CTX-07 (CTX-07):** Ressourcenmessung je Task und Dienst: RAM, VRAM, Kontexttokens, Modellaufrufe, Latenz, Durchsatz, Kosten, Wiederaufnahmekosten.

**Abnahme:** Ein laufender Hintergrundtask bleibt aktiv, während dieselbe
Instanz eine persönliche Anfrage beantwortet und einen autorisierten
Statusbericht liefert; danach erfolgt die korrekte Fortsetzung ohne zweiten Core
und ohne Kontextvermischung (TEST-06).

---

## Phase EXT-PRIV – Datenschutz und kontrollierte Informationsflüsse

**Priorität: P0 (PRIV-01 bis PRIV-05, PRIV-07), P1 (PRIV-06, PRIV-08 bis
PRIV-10), P2 (PRIV-11).** Siehe
[Zielbilderweiterung §19](ZIELBILD_ERWEITERUNG_2026-11-27.md).

- [ ] **EXT-PRIV-01 (PRIV-01):** Sicherheitsprofil S0–S4 als Default-Profil sowie Eigentümer, Zweck, Empfänger, Aufbewahrung und Verarbeitungsumgebung an Artefakten führen; `TrustLevel` bleibt getrennte Dimension.
- [ ] **EXT-PRIV-02 (PRIV-02):** Default-deny-Kette über Memory-Retrieval, Kontextbildung, Transform, Request, Reason, Act und Output; Prüfung jeweils vor der Offenlegung.
- [ ] **EXT-PRIV-03 (PRIV-03):** Konservative Vererbung der Schutzanforderungen auf abgeleitete Artefakte, Graphkanten, Embeddings, Caches, Zusammenfassungen und Logs; Herabstufung nur über autorisierte, geprüfte Transformation.
- [ ] **EXT-PRIV-04 (PRIV-04):** Secret-Broker mit kurzlebigen, zweck- und ressourcenbegrenzten Berechtigungen; keine Schlüssel in LLM-Kontexten oder gewöhnlichen Logs.
- [ ] **EXT-PRIV-05 (PRIV-05):** Sandbox-, Dateisystem-, Datenbank- und Egress-Regeln außerhalb der Modelle durchsetzen; Prompt-Injection, Fehlerausgaben und Telemetrie als Datenabflüsse behandeln.
- [ ] **EXT-PRIV-06 (PRIV-06):** Nachweisbare Löschung, Aufbewahrungsfristen, Zugriffsaudit und Datenherkunft einschließlich Indizes, Backups und abgeleiteter Daten.
- [ ] **EXT-PRIV-07 (PRIV-07):** Routing-Reihenfolge Zulässigkeit → Qualität → Kosten → Latenz erzwingen; lokale Ausführung ist nicht automatisch vertrauenswürdig.
- [ ] **EXT-PRIV-08 (PRIV-08):** Lokale Klassifikation, Pseudonymisierung, Aggregation, Generalisierung und minimale Kontextprojektion; Grenzen der Pseudonymisierung dokumentieren.
- [ ] **EXT-PRIV-09 (PRIV-09):** Abstraktionsvertrag für externes symbolisches Reasoning; erzeugter Code wird vor Ausführung geprüft und isoliert.
- [ ] **EXT-PRIV-10 (PRIV-10):** Lokale Rekonstruktion und unabhängige Verification der abstrahierten Lösung; bei fehlender Information auf zulässige lokale Verarbeitung wechseln.
- [ ] **EXT-PRIV-11 (PRIV-11):** Recherche zu Confidential Computing, Differential Privacy, sicherer Mehrparteienberechnung und homomorpher Verschlüsselung; keine Implementierung, keine unbelegte Privatheitsbehauptung.

**Abnahme:** Nicht autorisierte Daten bleiben auch über Retrieval,
Graphbeziehungen, Cache, Zusammenfassung und Logs unzugänglich (TEST-07).
Markierte Testdaten fließen bei nicht erlaubter Cloud-Verarbeitung weder
direkt noch abgeleitet ab; eine zulässige abstrahierte Aufgabe wird extern
bearbeitet und lokal rekonstruiert (TEST-08).

---

## Phase EXT-FORM – Formale Methoden (Lean)

**Priorität: P0 (FORM-01, FORM-02, FORM-03, FORM-04), P1 (FORM-05, FORM-06),
P2 (FORM-07).** Siehe
[Zielbilderweiterung §20](ZIELBILD_ERWEITERUNG_2026-11-27.md).

- [ ] **EXT-FORM-01 (FORM-01):** Acht formale Artefakttypen (`formal_definition`, `formal_assumption`, `formal_model`, `proof_obligation`, `proof`, `theorem`, `counterexample`, `formal_verification_report`) als versionierte Artefakte mit Schemas einführen.
- [ ] **EXT-FORM-02 (FORM-02):** Nachweisvertrag mit Aussage, Annahmen, Modell-/Planversion, Toolchain, erlaubten Axiomen und Prüfresultat; `sorry`, Timeouts und unprüfbare Ergebnisse gelten nicht als verifiziert.
- [ ] **EXT-FORM-03 (FORM-03):** Sollzustände, Vor-/Nachbedingungen, Invarianten und Ressourcenlimits formal ausdrücken und als strukturierte Reasoning-Rückmeldung auswerten.
- [ ] **EXT-FORM-04 (FORM-04):** Sicherheitsmodell des Core formalisieren (Authority-Erhalt, Referenzintegrität, Klassifikationsvererbung, Declassification, Zustandsübergänge, Ressourcengrenzen); Noninterference nur mit Beobachter- und Bedrohungsmodell.
- [ ] **EXT-FORM-05 (FORM-05):** Isolierten Lean-Prüfadapter als bestehende Capability betreiben; bei vorgeschriebenem, fehlgeschlagenem oder nicht verfügbarem Nachweis wird die Aktion blockiert oder in einen Wartezustand überführt.
- [ ] **EXT-FORM-06 (FORM-06):** Lean mit SMT-/Constraint-Solvern, Optimierern, Simulationen und Tests kombinieren; epistemischer Status der Ergebnisse getrennt führen.
- [ ] **EXT-FORM-07 (FORM-07):** Rust-zu-Modell-Übertragung evaluieren, ohne das extrahierte Modell mit dem Produktionssystem gleichzusetzen.

**Abnahme:** Ein gültiger Nachweis wird angenommen, ein falscher,
unvollständiger, veralteter oder auf unzulässigen Annahmen/Axiomen
basierender Nachweis wird abgewiesen; ein vorgeschriebener fehlender Nachweis
blockiert die Wirkung (TEST-09).

---

## Phase EXT-OPS – Dauerbetrieb, verteilte Ausführung, KI-Mitarbeiter

**Priorität: P0 (OPS-01, OPS-03), P1 (OPS-02, OPS-04, OPS-05, OPS-06).**
Siehe [Zielbilderweiterung §21](ZIELBILD_ERWEITERUNG_2026-11-27.md).

- [ ] **EXT-OPS-01 (OPS-01):** Host-Lebenszyklus mit Ereignissen, geplanten Aktivierungen, Heartbeats, Status, Recovery, begrenztem Retry und kontrolliertem Shutdown; keine LLM-Inferenz im Wartezustand.
- [ ] **EXT-OPS-02 (OPS-02):** Execution Nodes mit authentifizierter Identität, Fähigkeiten, OS, Kapazität, Trust, Gesundheitszustand und kurzlebigen Grants; logische Sternarchitektur bleibt.
- [ ] **EXT-OPS-03 (OPS-03):** Persistente Task Ownership mit Leases, Fencing und Operation IDs; Wiederaufnahme berücksichtigt erfolgte Wirkungen, unklare Wirkungen werden explizit geklärt.
- [ ] **EXT-OPS-04 (OPS-04):** Externe Agenten und Coding-Harnesses als begrenzte Capabilities mit strukturiertem Input, Status, Evidenz, Resultat, Abbruch und Ressourcenbudget.
- [ ] **EXT-OPS-05 (OPS-05):** Autorisiertes Operations-Interface für Aufgaben, Budgets, Kosten, Knoten, Fehler, Journal, Freigaben und menschliche Übernahme; Freigabe gilt nur für die geprüfte Operation.
- [ ] **EXT-OPS-06 (OPS-06):** Rollen- und Mitarbeiterprofil als persistente logische Identität mit Zielen, Policies und Arbeitshistorie ohne eigene Authority-Quelle.

**Abnahme:** Node- oder Host-Ausfall sowie verspätete Resultate führen
weder zur falschen Task-Zuordnung noch zur unkontrollierten Wiederholung einer
externen Wirkung (TEST-10).

---

## Phase EXT-BIZ – Unternehmensfähigkeit und Business-/Systems-Reasoning

**Priorität: P0 (BIZ-05), P1 (BIZ-01 bis BIZ-04, BIZ-06).** Siehe
[Zielbilderweiterung §22](ZIELBILD_ERWEITERUNG_2026-11-27.md).

- [ ] **EXT-BIZ-01 (BIZ-01):** Organisationsziele, Rollen, Budgets, Regeln, Stakeholder, Risiken, Fristen und Entscheidungsverantwortung als generische Artefakte und Policies modellieren.
- [ ] **EXT-BIZ-02 (BIZ-02):** Business-/Requirements-Reasoning trennt bestätigte Anforderungen, Annahmen, Hypothesen und offene Fragen und erzeugt Rückfragen.
- [ ] **EXT-BIZ-03 (BIZ-03):** Product-, UX- und Systems-Reasoning erzeugt Alternativen und prüft Abhängigkeiten, Akzeptanzkriterien, Wirtschaftlichkeit, Ressourcen und Wartbarkeit.
- [ ] **EXT-BIZ-04 (BIZ-04):** Eingehende Ereignisse werden zu autorisierten Aufgaben; Investigation, Umsetzung, unabhängige Tests, Freigabe und Deployment bleiben nachweisbare Schritte.
- [ ] **EXT-BIZ-05 (BIZ-05):** Drei Autonomiestufen pro Aktion als Policy erzwingen; Budget-, Vertrags-, Preis- und Risikogrenzen sind konfigurierbar; ARCS ersetzt keine gesetzlich erforderliche menschliche Verantwortung.
- [ ] **EXT-BIZ-06 (BIZ-06):** Formale Regeln, Entscheidungsmodelle, Simulationen und Evidenz absichern Entscheidungen; Erfolg an tatsächlichen Ergebnissen, Kosten und menschlichen Eingriffen messen.

**Abnahme:** Ein unvollständiger Kundenauftrag wird in belegte
Anforderungen, Hypothesen, Rückfragen, Plan, Umsetzung und überprüfte
Ergebnisse überführt; eine nicht autorisierte
Produktionsänderung oder Budgetüberschreitung wird blockiert (TEST-11).

---

---

## 3. Drei verbindliche Systemtests

### TEST-01 – Universelle Steuerung

**Aufgabe:** Eine Drohne erkundet einen unbekannten simulierten Raum und findet einen Gegenstand.

Geprüft werden dynamische Fähigkeitsauswahl, Sensorik, Planung, Action, Sicherheitsgrenzen und Ergebnisnachweis.

### TEST-02 – Langfristiges Softwareprojekt

**Aufgabe:** Eine Anwendung entwickeln, testen, verbessern und über zahlreiche getrennte Sitzungen fortsetzen.

Geprüft werden Aufgabenplanung, Werkzeugnutzung, Artifact-Gedächtnis, Wiederaufnahme, Fehlerbehandlung und Ressourcenoptimierung.

### TEST-03 – Gedächtnis und Wiederverwendung

**Aufgabe:** Mehrere ähnliche, aber nicht identische Aufgaben bearbeiten.

Geprüft wird, ob ARCS aus vergangenen Erfahrungen relevante Informationen wiederfindet und dadurch bessere oder günstigere Entscheidungen trifft, ohne fehlerhafte Erinnerungen ungeprüft zu übernehmen.

### Ergänzende Systemtests der Zielbilderweiterung (TEST-06 bis TEST-12)

Diese Testreihe schließtt an v3 §13 an und ersetzt die Tests
TEST-01 bis TEST-03 dieses Plans nicht. Die vollständige Beschreibung mit
Bezug auf die betroffenen Anforderungs-IDs steht in
[Zielbilderweiterung §23](ZIELBILD_ERWEITERUNG_2026-11-27.md).

- **TEST-06 – Assistenz während Unternehmensarbeit:** Hintergrundtask mit
  externem Prozess bleibt aktiv, während dieselbe Instanz eine persönliche
  Anfrage beantwortet und einen autorisierten Statusbericht liefert; danach
  korrekte Fortsetzung ohne zweiten Core oder Kontextvermischung.
- **TEST-07 – Kontrolliertes gemeinsames Memory:** Autorisierte
  bereichsübergreifende Abfrage gelingt; nicht autorisierte private oder
  fremde Unternehmensdaten bleiben über Retrieval, Graphbeziehungen, Cache,
  Zusammenfassung und Logs unzugänglich.
- **TEST-08 – Vertrauliches Reasoning:** Markierte Testdaten fließen bei
  nicht erlaubter Cloud-Verarbeitung weder direkt noch abgeleitet über Modell,
  Shell, Browser, Logs oder Netzwerk ab; eine zulässige abstrahierte Aufgabe
  wird extern bearbeitet und lokal rekonstruiert.
- **TEST-09 – Formale Prüfung:** Gültiger Nachweis wird angenommen;
  falsche, unvollständige, veraltete oder auf unzulässigen
  Annahmen/Axiomen basierende Nachweise werden abgewiesen; ein vorgeschriebener
  fehlender Nachweis blockiert die Wirkung.
- **TEST-10 – Verteilte Recovery:** Node-/Host-Ausfall und verspätete
  Resultate führen weder zur falschen Task-Zuordnung noch zur
  unkontrollierten Wiederholung einer externen Wirkung; unklare Wirkung wird
  explizit geklärt.
- **TEST-11 – Betrieblicher Prozess:** Ein unvollständiger Kundenauftrag
  wird in belegte Anforderungen, Hypothesen, Rückfragen, Plan, Umsetzung und
  überprüfte Ergebnisse überführt; eine nicht autorisierte
  Produktionsänderung oder Budgetüberschreitung wird blockiert.
- **TEST-12 – Ressourcen und Fairness:** RAM/VRAM, Tokenkosten,
  Antwortlatenz, Durchsatz und Wiederaufnahmekosten werden mit wachsender Zahl
  aktiver und wartender Kontexte gemessen und mit getrennten Vollinstanzen
  verglichen; Limits, faire Ausführung und interaktive
  Reaktionsfähigkeit werden nachgewiesen.

---

## 4. Nächste unmittelbar umzusetzende Arbeiten

Die erste Entwicklungsrunde sollte diese Reihenfolge haben:

1. **Bestehende Architektur absichern:** Manager-Zuständigkeiten dokumentieren, ohne sofort alles umzubauen.
2. **Artifact-Retrieval verbessern:** Einen echten erinnerungsbasierten Kontextaufbau für Reasoning implementieren.
3. **Lokales LLM im vorhandenen Goal-Loop testen:** Die bisherige Infrastruktur mit echter Inferenz statt nur regelbasierten Beispielen prüfen.
4. **Dauerhafte Aufgaben mit Wartezuständen ermöglichen:** Scheduler, Ereignistrigger und Wiederaufnahme ergänzen.
5. **Dynamische Capability-Auswahl als kleinen vertikalen Prototyp bauen:** Mindestens drei unabhängige Komponenten ohne fest verdrahtete Ablaufroute kombinieren.
6. **Einen schwierigen End-to-End-Test ausführen:** Ergebnisse, Fehler, Kosten und notwendige menschliche Eingriffe messen.

### Ergänzung vom 27. November 2026

Für die Zielbilderweiterung gilt die in
[Zielbilderweiterung §23.2](ZIELBILD_ERWEITERUNG_2026-11-27.md)
empfohlene Reihenfolge. Sie beginnt mit dem Vertragsabgleich (Phase EXT-0)
und lässt die bisherigen Arbeiten unberührt weiterlaufen:

1. **Vertragsabgleich (EXT-0):** Bestehende Goal-Runtime, Task-Queues,
   Journale, Ressourcenreservierungen, Memory-Scopes, Admission-Regeln und
   Adapterverträge den Anforderungen zuordnen; keine parallele
   Ersatzarchitektur.
2. **Mehrere persistente Arbeitskontexte (EXT-CTX):** Gemeinsame
   Infrastruktur mit autorisierten systemweiten Statusabfragen; Abnahme
   TEST-06.
3. **Sicherheitsslice Datenschutz (EXT-PRIV):** Datenflussprüfung, Herkunft,
   Secret-Broker, Egress-Isolation und zulässigkeitsbasiertes Modellrouting;
   Abnahme TEST-07 und TEST-08.
4. **Formales Kernmodell mit isoliertem Lean-Adapter (EXT-FORM):** Positive
   und negative Nachweise; Modellgrenzen dokumentieren; Abnahme TEST-09.
5. **Ereignisbasierte betriebliche Aufgaben und verteilte Ausführung
   (EXT-OPS, EXT-BIZ):** Task Ownership mit Leases, externe Harnesses als
   begrenzte Capabilities; Abnahme TEST-10 und TEST-11.

## 5. Entwicklungsregeln

- Keine neuen zentralen Steuerungsschichten.
- Keine zweite unabhängige Gedächtnisdatenbank als Wahrheitsquelle.
- Keine unnötige Duplizierung gemeinsamer Adapterlogik.
- Keine gerätespezifischen Sonderfälle im Core.
- Keine ungeprüfte Ausführung von Modellvorschlägen.
- Jede neue Architekturabstraktion braucht einen nachvollziehbaren praktischen Nutzen.
- Jede neue Funktion muss mindestens einen positiven, einen negativen und einen Wiederaufnahmetest bestehen.
- Für delegierte Doku und Routinecode gelten die [Metaprompts](METAPROMPTS.md) mit einem Nachweis im jeweiligen Zwischenbericht.

**Langfristige Definition des Erfolgs:** ARCS versteht eine Aufgabe, findet oder entwickelt passende Fähigkeiten, plant, handelt, überprüft, erinnert sich an Ergebnisse und verfolgt seine Ziele über längere Zeiträume – mit möglichst wenig notwendiger Benutzerkonfiguration.
