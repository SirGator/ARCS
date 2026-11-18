# Bewertungen und Gültigkeit von Erinnerungen

`ARCHITECTURE.md` v3 §6.4/§6.5 verlangt eine Trennung zwischen Abrufrelevanz,
Herkunft und fachlicher Gültigkeit. Der Core kann jetzt eine **konkrete
Artifact-Version** bewerten lassen, ohne ihren Inhalt oder Trust zu verändern.
Der neue Dienst verwendet ausschließlich den bestehenden Artifact Store.

## Startbares Beispiel

```bash
cargo build --workspace --bins --examples --locked --offline
./target/debug/examples/memory-assessments \
  apps/adapter-host/config/memory-assessments/host.json \
  apps/adapter-host/config/memory-assessments/runtime.json
```

Zwei JSON-Input-Prozesse liefern vier Notizen und eine Zahlenaufgabe. Das
Beispiel setzt ausdrücklich zwei synthetische Betreiberbewertungen: eine
aktive und eine zurückgezogene Notiz. Die strikte Memory-Policy lässt die
aktive Notiz zu und überspringt die zurückgezogene sowie eine unbewertete
Notiz. Eine weitere Notiz gehört zu einem nicht freigegebenen Projekt.
Nur das konfigurierte `body` gelangt neben der Aufgabe an den externen Solver.

Das Ergebnis enthält einen Review-Auftrag, einen Reasoning-Versuchsbeleg,
zwei Bewertungen und die Kontextauswahl mit drei Abrufentscheidungen. Ein
Wiederholungsstart verwendet dieselben Belege. Die getrennten Journale liegen
unter `var/memory-assessments/`. Der Solver nutzt die Notiz nicht fachlich;
dies ist ein Nachweis für Auswahl, Feldgrenzen und Replay, kein Nachweis einer
verbesserten allgemeinen Problemlösungsqualität.

## Betreiberbefehle

Ein authentifizierter Betreiber kann über den lokalen Host eine Bewertung
speichern. Die tatsächlichen Versions-IDs stammen aus dem vorhandenen Core.

```json
{
  "command": "assess",
  "assessment": {
    "target_version_id": "SOURCE_VERSION_ID",
    "expected_version_id": null,
    "decision_by": {"actor_type": "human", "id": "simon"},
    "reason": "Für diesen Zeitraum anhand des Testbelegs geprüft",
    "status": "active",
    "valid_from": "2026-11-18T00:00:00Z",
    "valid_until": "2027-02-01T00:00:00Z",
    "evidence": ["TEST_EVIDENCE_VERSION_ID"]
  }
}
```

```bash
./target/debug/arcs-host --config HOST.json --memory-command assessment.json
```

Beim ersten Eintrag ist `expected_version_id` leer oder fehlt. Für eine
Änderung muss sie die letzte Bewertungs-Version nennen. Der Befehl gibt diese
Version und seinen gespeicherten Inhalt zurück. Zwei konkurrierende Änderungen
können sich nicht überschreiben. Eine exakte Wiederholung des jüngsten Befehls
liefert denselben Beleg. Nach einer weiteren Änderung wird ein alter Befehl
als Konflikt abgewiesen; er wird nicht erneut angewendet.

Eine neue Bewertung enthält den vollständigen gewünschten Zustand. Zum
Widerruf wird `status: "retracted"` mit der erwarteten Bewertungs-Version
gesendet. Frühere Bewertungen und die ursprünglichen Quellen bleiben erhalten.

```json
{
  "command": "inspect",
  "target_version_ids": ["SOURCE_VERSION_ID", "ANOTHER_SOURCE_VERSION_ID"]
}
```

Die Abfrage nimmt 1–32 unterschiedliche IDs entgegen. `assessment: null`
bedeutet, dass keine Bewertung bekannt ist. Ein fehlerhafter oder zu großer
Bewertungsbeleg wird als Fehler gemeldet. Er wird nicht als fehlende Bewertung
behandelt. Die CLI prüft Syntax, Rolle und Grenzen vor dem Hoststart;
Befehlsdateien sind auf 65.536 Bytes begrenzt. Sie startet keinen Tasklauf.
Die konfigurierten Adapter werden wie bei anderen Host-Abfragen für ihre
Metadaten geöffnet; eine aktivierte Reconnect-Policy kann dabei
Verbindungsmetadaten speichern.

Die lokale CLI setzt Zugriff auf das Hostkonto voraus. `decision_by` ist ein
Auditfeld und keine Anmeldung. Eingebettete Anwendungen müssen die Identität
und ihre Berechtigung vor `Host::assess_memory` beziehungsweise dem Core-Dienst
authentifizieren. Externe Adapter können den reservierten Publisher
`arcs.memory.assessment` nicht registrieren; Reasoner erhalten keine solche
Schreibfähigkeit. Modellvorschläge werden nicht automatisch übernommen.

## Zustände und Abrufpolicy

| Zustand | Bedeutung für die optionale Policy |
| --- | --- |
| Keine Bewertung | Nur mit `allow_unassessed: true` zulassen |
| `active` | Innerhalb der angegebenen Zeitgrenzen zulassen |
| `disputed` | Innerhalb der Zeitgrenzen nur mit `allow_disputed: true` zulassen |
| `retracted` | Auslassen |
| `superseded` | Auslassen; eine andere Version ist als Ersatz genannt |

`active` dokumentiert eine Betreiberentscheidung, keinen automatischen
Wahrheitsbeweis. Gründe und bis zu 16 unterschiedliche Belegversionen bleiben
nachvollziehbar. Ein Ersatz muss eine andere existierende Version desselben
Schemas sein. Der Core importiert ihn nicht automatisch: Der normale Scope,
die Feldauswahl und die Budgets müssen ihn gesondert zulassen.

```json
{
  "memory": {
    "epistemic": {"allow_unassessed": false, "allow_disputed": false},
    "scopes": [],
    "relations": [],
    "search": {"max_depth": 2, "max_visited": 64, "max_edges": 128, "max_candidates": 16},
    "max_items": 2,
    "max_bytes": 2048
  }
}
```

`scopes` ist hier ein Platzhalter und muss wie im
[Abrufbeispiel](artifact-memory.md) ausdrücklich befüllt werden. Ohne
`epistemic` bleibt der bisherige Abrufvertrag bestehen. Mit der Option gilt
die Bewertung für zusätzliche Erinnerungen nach der begrenzten Graph-/Textsuche
und vor der Kontextzusammenstellung. Sie erweitert keine Zugriffsrechte.
Die Suchgrenze bleibt bindend: Ausgeschlossene Treffer lösen keine unbegrenzte
Suche nach weiteren Erinnerungen aus. Ein fehlender zugelassener Treffer lässt
den Pflichtkontext unverändert.

Die Zeitprüfung verwendet die injizierbare Core-Clock, im normalen Host die
UTC-Systemzeit. Eine nur für den Scheduler simulierte Tick-Zeit ersetzt diese
Clock nicht.

Datumsgrenzen verwenden exakt `YYYY-MM-DDTHH:MM:SSZ`, UTC ohne Bruchteile.
Der Beginn gilt einschließlich, das Ende ausschließlich. Nicht begonnene und
abgelaufene Informationen werden ausgelassen. `retracted` und `superseded`
wirken unabhängig von solchen Grenzen sofort als Ausschluss. Eine spätere
aktive Bewertung kann einen Widerruf ausdrücklich revidieren.

Bewertungen sind versionsbezogen. Eine neue Version derselben Quelle erbt
die Bewertung nicht. Bloße `contradicts`-/`supersedes`-Kanten und hohe
Netzaktivierungen ersetzen keine Bewertung. Quellversions-ID und Trust gelangen mit den
normalen Feldern an den Reasoner. Bewertungsgrund, Betreiber und Beleg-Payloads
werden nicht zusätzlich exportiert. Auch bei ausdrücklich erlaubten strittigen
Erinnerungen bleibt deren Bewertungsmetadatum im Core; es ist kein neues
Modell-Wirefeld. Die strikte Policy ist für aktuelle Fakten vorgesehen.

## Dauerhafte Kontextauswahl

Mit dieser Option entsteht `arcs.context_selection.v2`. Sie enthält den
Auswertungszeitpunkt und pro begrenztem Suchtreffer Zielversion,
Bewertungs-Version, Einordnung und Zulassungsentscheidung. Die Bewertung wird
an der festen Artifact-Commit-Grenze der Suche gelesen. Spätere Änderungen
beeinflussen diesen Snapshot nicht. Auch ausgeschlossene Quellen und ihre
Bewertungen werden über Provenienz und `based_on` referenziert.

Beim Replay prüft der Core die gespeicherten Entscheidungen erneut gegen
dieselben unveränderlichen Versionen, denselben Logcursor und denselben
Auswertungszeitpunkt. Neue Requests und Goal-Runden bewerten die inzwischen
bekannte Lage neu. Policywechsel unter derselben Request-ID werden abgewiesen.
Bestehende V1-Verträge und deren gespeicherte Payloads werden nicht geändert.

Ein Widerruf ändert keinen schon laufenden Auftrag und widerruft keine
Action-Freigabe. Für einen laufenden Task stehen die bestehenden
[Pause-/Abbruchbefehle](task-pauses.md) zur Verfügung. Verification, Approval
und aktuelle Gerätevorbedingungen bleiben eigene Prüfungen.
Auch ein fortgesetzter identischer Request behält seinen Snapshot; eine frische
Auswahl benötigt eine neue Request-ID, etwa die nächste Goal-Runde.

Automatische Erkennung inhaltlicher Widersprüche, semantische Konsolidierung,
kalibrierte Aussagewahrscheinlichkeiten und allgemeine Neuplanung bei
Wissensänderungen bleiben offen.
