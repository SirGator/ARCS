# ARCS – Implementierungsplan v2.0

> Arbeitsgrundlage vom 15. November 2026. Die Checkboxen beschreiben die
> Gesamtabnahme der jeweiligen Aufgabe, nicht nur erste Teilimplementierungen.
> Architekturvertrag: [ARCHITECTURE.md](ARCHITECTURE.md).
> Gesamtziel und künftige Anforderungen:
> [Technische Gesamtarchitektur v3.0](ARCHITECTURE.md).
> Anforderungs-IDs bleiben versions- und abschnittsbezogen; die bestehenden
> v2-Aufgaben und Abnahmestände werden durch v3 nicht umnummeriert.
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

- [ ] **RSN-01:** Ein echtes lokales LLM mit ungefähr 14B Parametern über Reasoning V2 testen.
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

---

## 4. Nächste unmittelbar umzusetzende Arbeiten

Die erste Entwicklungsrunde sollte diese Reihenfolge haben:

1. **Bestehende Architektur absichern:** Manager-Zuständigkeiten dokumentieren, ohne sofort alles umzubauen.
2. **Artifact-Retrieval verbessern:** Einen echten erinnerungsbasierten Kontextaufbau für Reasoning implementieren.
3. **Lokales LLM im vorhandenen Goal-Loop testen:** Die bisherige Infrastruktur mit echter Inferenz statt nur regelbasierten Beispielen prüfen.
4. **Dauerhafte Aufgaben mit Wartezuständen ermöglichen:** Scheduler, Ereignistrigger und Wiederaufnahme ergänzen.
5. **Dynamische Capability-Auswahl als kleinen vertikalen Prototyp bauen:** Mindestens drei unabhängige Komponenten ohne fest verdrahtete Ablaufroute kombinieren.
6. **Einen schwierigen End-to-End-Test ausführen:** Ergebnisse, Fehler, Kosten und notwendige menschliche Eingriffe messen.

## 5. Entwicklungsregeln

- Keine neuen zentralen Steuerungsschichten.
- Keine zweite unabhängige Gedächtnisdatenbank als Wahrheitsquelle.
- Keine unnötige Duplizierung gemeinsamer Adapterlogik.
- Keine gerätespezifischen Sonderfälle im Core.
- Keine ungeprüfte Ausführung von Modellvorschlägen.
- Jede neue Architekturabstraktion braucht einen nachvollziehbaren praktischen Nutzen.
- Jede neue Funktion muss mindestens einen positiven, einen negativen und einen Wiederaufnahmetest bestehen.

**Langfristige Definition des Erfolgs:** ARCS versteht eine Aufgabe, findet oder entwickelt passende Fähigkeiten, plant, handelt, überprüft, erinnert sich an Ergebnisse und verfolgt seine Ziele über längere Zeiträume – mit möglichst wenig notwendiger Benutzerkonfiguration.
