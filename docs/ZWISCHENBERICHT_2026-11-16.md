# ARCS – Zwischenbericht zur Architekturimplementierung

**Stand: 16. November 2026 · Ausgangscommit: `23d97ef`**

ARCS hat jetzt eine persistente, begrenzt parallele Aufgabenverarbeitung mit
Artifact-Gedächtnisabruf, Termin- und Ereignisaktivierung sowie dynamisch
vorgeschlagenen Teilplänen. Solche Pläne können neben Reasoning-Schritten auch
freigegebene Goal-Vorlagen instanziieren. Deren konkrete Aktionen durchlaufen
die bestehende Verification und benötigen menschliche Freigaben.

Die Umsetzung folgt der maßgeblichen [ARCHITECTURE.md](ARCHITECTURE.md).
Der [Implementierungsplan](IMPLEMENTATION_PLAN.md) beschreibt die neun bisherigen
Entwicklungsschritte und die weiterhin offenen Gesamtabnahmen. Das langfristige
Ziel bleibt ein universeller persönlicher Assistent, der unterschiedliche
Systeme verbindet und Projekte über Monate verfolgt.

## 1. Umgesetzte Funktionen

| Bereich | Vorhandenes Verhalten | Funktionsnachweis |
| --- | --- | --- |
| Paralleles Reasoning | Begrenzte Worker, unabhängige Aufträge, exklusive Adapter-Sessions, Deadlines und isolierte Fehler | [Parallele Runtime](../apps/adapter-host/examples/parallel-runtime.md) |
| Artifact-Gedächtnis | Begrenzter Metadaten-/Graphabruf innerhalb erlaubter Scopes; eingefrorene Kontextauswahl für Wiederaufnahme | [Artifact-Memory](../apps/adapter-host/examples/artifact-memory.md) |
| Prozessumgebung | Ausdrückliche Umgebungsfreigaben pro Adapter; gezielte Weitergabe von Modellschlüsseln auch im Kontrollpanel | [Prozessumgebung](../apps/adapter-host/examples/process-environment.md) |
| Anfangsaktivierung | Persistente Termine, relative Wartezeiten und Ereignistrigger; Weckereignisse kommen auch bei voller Queue an | [Scheduler](../apps/adapter-host/examples/scheduled-runtime.md) |
| Wiederholte Zielrunden | Dauerhafte Wartephasen zwischen bewerteten Runden; neue Daten und neue Freigaben nach dem Wecken | [Goal-Pausen](../apps/adapter-host/examples/goal-pauses.md) |
| Abhängigkeiten | Parallele Zweige mit anschließender Zusammenführung; explizite Ergebnisfelder und gespeicherte Versionsbezüge | [Teilaufgaben](../apps/adapter-host/examples/task-dependencies.md) |
| Dynamische Pläne | Externe Vorschläge aus freigegebenen Vorlagen; Graph-/Budgetprüfung, unveränderliche Planartefakte und atomare Aufnahme | [Dynamische Pläne](../apps/adapter-host/examples/dynamic-plans.md) |
| Planabbruch | Dauerhafter Abbruch offener Planner und Kindaufträge; fertige Ergebnisse bleiben erhalten, späte Antworten werden verworfen | [Abbruchvertrag](../apps/adapter-host/examples/dynamic-plans.md#einen-plan-gezielt-abbrechen) |
| Goal-Schritte in Plänen | Datenbeschaffung, Handlungsvorschlag, Verification, manuelle Freigabe und Wirkungsbewertung über die bestehende Zielschleife | [Goal-Pläne](../apps/adapter-host/examples/goal-plans.md) |

## 2. Architektur und Zuständigkeiten

- **Ein logischer Core und ein Artifact-Netz:** Kontextauswahl, Vorschläge,
  angenommene Pläne und Wirkungsbelege verwenden den vorhandenen Core-Store.
  Runtime-Journale halten operative Queue-, Scheduler- und Recovery-Zustände;
  sie bilden keine zweite unabhängige Wissensbasis.
- **Bestehende Manager-Verträge:** Reasoning, Input, Observe, Request, Act und
  Output behalten ihre Zuständigkeiten und dieselben Grants. Der Planer erhält
  durch seine Koordinationsrolle keine zusätzliche Handlungsbefugnis.
- **Externe Intelligenz:** Solver, Rules-Reasoner und Modelladapter bleiben
  eigenständige Komponenten. Gerätewissen liegt in externen Adaptern und
  Beispielkonfigurationen.
- **Getrennte Implementierungsmodule:** Reason-Worker, Input-Abholung,
  Abhängigkeiten, Budgetprüfung, Planveröffentlichung, Scheduler und
  Journalpersistenz sind nach Zuständigkeit getrennt. Planabschluss und Joins
  verwenden dieselbe Ergebnisprüfung.

## 3. Wichtige Ausführungsverträge

1. Ein Modellvorschlag wird vor der Aufnahme gegen Betreiber-Vorlagen,
   Abhängigkeiten, Ergebnisexporte und Gesamtbudgets geprüft. Freie Programme,
   Aktionsargumente, neue Grants und verschachtelte Pläne sind nicht Teil dieses
   Vorschlagsvertrags.
2. Angenommene Pläne werden als `arcs.plan.v1` mit eingefrorenen Routen und
   Bezügen zur Eingabe, zum Vorschlag und zur Runtime-Definition gespeichert.
   Ihr Trust bleibt niedrig; ein gültiger Plan bestätigt keine Modellbehauptung.
3. Vorab reservierte Queueplätze verhindern, dass mehrere Planner ihre eigene
   Erweiterung gegenseitig blockieren. Kindaufträge und Abhängigkeiten werden
   gemeinsam in einer Journaltransaktion aufgenommen.
4. Wiederaufnahme verwendet stabile Invocation-IDs und gespeicherte Kontexte.
   Die Vermeidung doppelter externer Wirkungen setzt weiterhin einen Adapter
   voraus, der seinen Idempotenz- und Replay-Vertrag erfüllt.
5. Für dynamisch gewählte Goals ist manuelle Freigabe Pflicht. Die Gesamtbudgets
   berücksichtigen alle maximalen Runden, Planungs-/Bewertungsaufrufe und
   möglichen Action-Dispatches. Jeder konkrete Kandidat wird gesondert geprüft.
6. Ein Goal exportiert erst seine abschließende Bewertung nach `completed`.
   Eine Freigabe oder Ausführungsquittung allein löst keinen erfolgreichen Join aus.
7. Neue Goal-Instanzen beginnen ihre Beobachtungsgrenze bei der Datenerfassung.
   Nach Wartephasen werden frische Belege erfasst; eine alte Kandidatenfreigabe
   kann keine spätere Runde autorisieren.
8. Abbruch stoppt zukünftige Arbeit und verhindert die Wiederbelebung durch
   verspätete Freigaben. Bereits gesendete Berechnungen behalten ihre
   Ressourcenplätze bis Antwort oder Deadline. Bereits erfolgte Gerätewirkungen
   werden dadurch nicht zurückgenommen.

`ready_for_review` des Gesamtplans bedeutet: Die geplanten Schritte haben ihre
jeweiligen Abschlussverträge erfüllt. Ein allgemeiner Prüfer für die Erfüllung
beliebiger Benutzerziele ist damit noch nicht implementiert.

## 4. Prüfergebnisse

Der zuletzt vollständig geprüfte Implementierungsstand umfasst:

| Prüfung | Ergebnis |
| --- | --- |
| `cargo test --workspace --locked --offline` | 419 Tests bestanden, keine fehlgeschlagen |
| Kontrollpanel: `python3 -m unittest discover -s apps/control-panel/tests -v` | 40 erfasst: 29 bestanden, 11 umgebungsabhängige Desktoptests übersprungen |
| `cargo build --workspace --bins --locked --offline` | Erfolgreich |
| `cargo clippy --workspace --all-targets --locked --offline` | Erfolgreich; zwei bereits vorhandene Warnungen in `reasoning/service.rs` |
| `cargo fmt --all --check` und `git diff --check` | Erfolgreich |

Die Prozess-Tests verwenden echte externe Adapterprozesse und persistente
Journale. Geprüft sind unter anderem parallele Zweige, volle Queues,
fehlerhafte Pläne, isolierte Ausfälle, verlorene Antworten, fehlgeschlagene
Commits, Migrationen, Abbruch, erneute Freigaben und simulierte Monatswartephasen.

Das CLI-Beispiel mit dem simulierten Ventilator wurde zusätzlich durchlaufen:
Vor Freigabe gab es **0 Aktionen**, nach Ausführung und weiterem Neustart
**genau 1 Aktion**. Der Workflow-Planer wurde **einmal** aufgerufen;
abgeschlossene Schritte wurden nicht erneut ausgeführt.

Diese Nachweise ersetzen keinen realen mehrmonatigen Dauerbetrieb. Die neuen
Planungsbeispiele verwenden deterministische Rules-Reasoner beziehungsweise
Testadapter; sie belegen keine universelle Planung durch ein echtes LLM.

## 5. Weiterhin offene Arbeit

- **Allgemeine Planung:** Freie Zielzerlegung und Capability-Suche, Neuplanung,
  Alternativen, rekursive Teilpläne und ein allgemeiner Gesamtzielprüfer.
- **Langfristiges Zielmodell:** Versionierte Projekte, Teilziele, Fristen,
  Fortschritt und Entscheidungen im Core; Pause/Resume laufender Pläne und
  ein Live-Steuerkanal für die Runtime.
- **Ressourcenkoordination:** Allgemeine Reservierungen für Geräte, GPU, RAM
  und APIs sowie umfassende Prioritäts-, Fairness- und Lastmessungen.
- **Weitere Parallelität:** Allgemein parallele Request-, Act- und Output-Ports;
  diese sind durch paralleles Reasoning noch nicht vollständig abgedeckt.
- **Gedächtnisqualität:** Semantische Suche, Konsolidierung, überprüfbare
  Erfahrungswiederverwendung und Benchmarks bei stark wachsender Historie.
- **Autonomie und Isolation:** Automatische Freigaben in erzeugten Plänen,
  direkte Act-Knoten, sichere Werkzeugentwicklung und Betriebssystem-Sandboxing.
- **Reale Inferenz und Robotik:** Anspruchsvolle End-to-End-Aufgaben mit echten
  Modellen und getrennte Integration von Drohnen-/Robotikadaptern. Die neue
  Goal-Plan-Demo steuert ausschließlich einen simulierten Ventilator.

Die Gesamtabnahmen aus `ARCHITECTURE.md` und dem Implementierungsplan bleiben
offen, soweit dort mehr als diese begrenzten Teilimplementierungen gefordert ist.

## 6. Empfohlene nächste Entwicklungsschritte

1. Den vorhandenen Planvertrag mit einem echten externen Modell an mehreren
   überprüfbaren Aufgaben testen und Fehler, Laufzeit sowie Kosten erfassen.
2. Allgemeine Ressourcenreservierungen vor einer Ausweitung gleichzeitig
   steuernder Geräteaufgaben ergänzen.
3. Ein versioniertes Zielmodell und kontrollierte Neuplanung auf den vorhandenen
   Planartefakten und Ergebnisbelegen aufbauen.
4. Einen anspruchsvollen Simulator-Test und anschließend einen Dauerbetrieb mit
   realen Unterbrechungen durchführen.
