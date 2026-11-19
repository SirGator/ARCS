# ARCS – Zwischenbericht zur versionierten Neuplanung, 19. November 2026

Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md), insbesondere
HET-06, die versionierten DAGs in §5.3, PAR-09/PAR-10/PAR-11 und
LNG-02/LNG-05/LNG-06/LNG-07. Dieser Slice ergänzt die vorhandene Plan-Engine
um begrenzte Neuplanung nach einem nicht erfolgreich abgeschlossenen Schritt.
Die vollständigen allgemeinen Phasenabnahmen bleiben offen.

## Umsetzung

Die optionale Policy `plan.replanning.max_revisions` erlaubt 1–7 zusätzliche
Vorschläge. Der ursprüngliche Root-Auftrag und seine Eingabeversion bleiben
erhalten. Angenommene Pläne liegen als unveränderliche `arcs.plan.v2`-Artefakte
mit Revisionsindex und Vorgängerbezug im vorhandenen Artifact-Netz. Der
Vorschlagsvertrag `arcs.plan_candidate.v1` und bisherige einmalige Planrouten
bleiben nutzbar. Der neue Vertrag erweitert den bisherigen v1-Vertrag nicht
stillschweigend um neue Bedeutungen.

Jede Revision erhält eigene Schritt- und Request-Identitäten. Vorherige
Ergebnisse, Freigaben und externe Wirkungen werden nicht als neue Aufträge
umetikettiert. Im Bericht sind die angenommenen Revisionen und deren Schritte
sichtbar. Erfolgreiche alte Schritte bleiben erhalten; neu vorgeschlagene
Schritte sind eigenständige Aufträge innerhalb derselben ursprünglichen Aufgabe.

Der Übergang speichert den Rückblick und stoppt offene alte Zweige in einer
Journaltransaktion. Erst danach verwirft der Host zugehörige ausstehende
Antworten. Das Core-Artefakt `arcs.plan_replanning.v1` dokumentiert den
begrenzten Rückblick mit Schrittzuständen, Auflösungsstatus und Ergebnisbezügen.
Die neue Planung bekommt nur freigegebene `export_fields` tatsächlich
abgeschlossener Schritte. Vollständige private Vorlagen und rohe Adapterfehler
werden nicht weitergegeben. Überschüssige Kontextpositionen werden gezählt;
die vorhandenen Byte- und Memory-Grenzen gelten zusätzlich.

## Budgets und Ausführung

Bei aktivierter Neuplanung zählen alle bisherigen zugelassenen Schritte und
sämtliche Planungsrunden zum selben konservativen Gesamtbudget. Konfigurierte
Reasoner-Alternativen, Goal-Runden, Initialchecks und Assessments werden
berücksichtigt. Aufgegebene Zuteilungen werden nicht erstattet. Ohne genügend
Planungsbudget entsteht kein weiterer Modellaufruf; ein übergroßer neuer
Vorschlag erzeugt keine weiteren Kinder.

Die Root-Aufgabe behält ihre ursprüngliche Lebensdauer. Neue Schritte erben
die frühere Frist. `1 + max_steps` Queueplätze bleiben für die Planfolge
reserviert; Kindschritte werden darin nicht doppelt gezählt. Damit kann ein
Plan auch bei voller Queue neu expandieren. Rechenplätze werden dadurch nicht
über Wartephasen hinweg belegt.

Goal-Vorlagen für Neuplanung verlangen ausdrückliche exklusive
Ressourcenidentitäten und manuelle Freigaben. Ein Ersatz-Goal erfasst neue
Quellen, erzeugt eine neue Candidate-Version und benötigt eigene Verification
sowie eine eigene Freigabe. Eine alte Freigabe kann die neue Goal-Sequenz
nicht autorisieren. Die bestehenden Regeln für ungeklärte externe Wirkungen
und Gerätebesitz bleiben wirksam.

## Wiederaufnahme und Zuständigkeiten

Operative Tabellen speichern Revisionsbezüge und gefrorene Checkpoints;
Planinhalte und Ergebnisartefakte bleiben im gemeinsamen Store. Es gibt
keinen zusätzlichen übergeordneten Controller. Neuplanung und Budgetrechnung
liegen in eigenen Modulen der vorhandenen Runtime; Publisher und Registry
verwenden weiterhin die bestehenden Input-/Artifact-Verträge.

Ein verlorener Modellreply wird mit derselben revisionsgebundenen Request-ID
wiederaufgenommen. Ein bereits im Core gespeicherter Ersatzplan wird nach
einem verlorenen Queue-Commit wiederverwendet. Historische Planstände prüfen
den tatsächlich belegten Produzenten ihrer jeweiligen Revision, auch wenn ein
späterer Aufruf einen anderen Fallback-Reasoner verwendet.

Core- und Journalfehler werden als solche weitergegeben. Die Runtime schreibt
bei einem fehlgeschlagenen Checkpoint keinen erfundenen terminalen
fachlichen Fehler über den bisherigen Zustand. Tatsächliche Komponentenfehler
bleiben isolierte Taskfehler; scheitert der neue Planer, bleibt kein scheinbar
weiterlaufender Replan im Bericht stehen.

Die gemeinsamen Testpeers besitzen jetzt explizite Optionen für Identität,
Journal, Zielvertrag, Projektionen und Fehlverhalten. Betroffene bestehende
Tests müssen ihren Python-Quelltext dafür nicht mehr per Textsuche umschreiben.
Ihre bisherigen Assertions zu Herkunft, Berechtigungen und selektivem Export
bleiben erhalten.

## Prüfungen und Ergebnis

Die neue Prozesssuite prüft positive, negative und Wiederaufnahmefälle:

- Neue Planstände bei derselben Originalaufgabe und unveränderten alten Artefakten.
- Begrenzte Revisionen sowie kumulative Laufzeit-, Token- und Action-Budgets.
- Zulassung bei voller Queue ohne Vermischung getrennter Eingaben.
- Gefilterte Ergebnisse und Abweisung privater Planpayloads im Replan-Kontext.
- Verlorene Antworten und Fehler an Intent-, Kontext- und Queue-Commitgrenzen.
- Revisionsgenaue Fallback-Produzenten nach Neustart.
- Pause, ursprüngliche Fristen und verworfene späte alte Antworten.
- Neue Quellen und eigene Freigaben für Ersatz-Goals mit genau zwei
  beobachteten Simulatorwirkungen und unverändertem Abschluss nach Neustart.

Alle **634 Workspace-Tests** sind erfolgreich, darunter die 21 neuen
Neuplanungs-Prozesstests und der Core-Vertragstest zur Trennung von v1 und v2
sowie zur Revisionsgrenze. Der nach einer expliziten Journalnamenänderung
erneut ausgeführte Ressourcen-/Abbruchtest ist ebenfalls erfolgreich.
`cargo build --workspace --bins --examples --locked --offline`,
`cargo fmt --all -- --check` und `git diff --check` sind erfolgreich.
Clippy über alle Workspace-Targets ist erfolgreich und meldet ausschließlich
die beiden bereits vorhandenen Warnungen in `reasoning/service.rs`.

Das native Beispiel wurde mit den neu gebauten Rust-Adaptern ausgeführt.
Es erzeugt zwei angenommene Planrevisionen, einen eingefrorenen
Neuplanungskontext und acht Reasoning-Versuche. Die erste Vorlage endet nach
dem Einschalten mit `budget_exhausted`; die zweite Vorlage erreicht mit
frischen Quellen nach dem Ausschalten das ursprüngliche Ziel. Zwei explizite
Testfreigaben des lokalen Simulators erlauben genau zwei beobachtete
Zustandswechsel. Diese Testfreigaben sind kein Nachweis einer menschlichen
Produktionsfreigabe. Nach erneutem Öffnen bleiben Plan-, Execution- und
Outcome-Versionen sowie die Zahl der Reasoning-Versuche und Wirkungen
unverändert. Die Auswertung ordnet Goal-Berichte über ihre Sequenz zu und
setzt keine Berichtsreihenfolge voraus. Lokale Protokolle und
`native-audit.json` liegen unter `var/verification-replanning/`.

## Grenzen

[Das ausführbare Beispiel](../apps/adapter-host/examples/plan-replanning.md)
verwendet externe deterministische Rules-Komponenten. Dieser Slice behauptet
keine neue allgemeine LLM-Strategiequalität und keine universelle automatische
Werkzeugkomposition. Auswahl und Argumente bleiben auf autorisierte Vorlagen
begrenzt. Der aktuelle Trigger ist ein fehlgeschlagener beziehungsweise
blockierter Schritt; beliebige Änderungen der Außenwelt sind noch keine
allgemeine Replan-Policy.

Vorherige Ergebnisse können Planungskontext sein; fertige alte Teilschritte
werden noch nicht als wiederverwendbare neue DAG-Knoten importiert.
Geldabrechnung, harte GPU-/RAM-Grenzen, verteiltes Controller-Fencing und
langfristiger realer Betrieb bleiben weitere Architekturarbeiten. Das
Simulatorbeispiel ersetzt keine Prüfung einer physischen Drohne.
