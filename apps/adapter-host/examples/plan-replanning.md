# Begrenzte Neuplanung mit unveränderter ursprünglicher Aufgabe

Eine Planroute kann nach einem nicht erfolgreich abgeschlossenen Teilschritt
einen neuen Vorschlag anfordern. Die optionale Betreiberpolicy lautet:

```json
{
  "plan": {
    "max_steps": 3,
    "max_total_runtime_ms": 100000,
    "replanning": { "max_revisions": 2 }
  }
}
```

Dies ist ein Ausschnitt; Vorlagen und die übrigen Routenfelder bleiben nötig.
`max_revisions` akzeptiert 1–7 zusätzliche Planungen. Ohne die Policy bleibt
es bei der bisherigen einmaligen Planung. Neuplanung nutzt dieselbe Fehlerisolation und dieselben Worker-Limits wie
die übrige Runtime.
Goal-Vorlagen müssen manuelle Freigaben und ausdrückliche exklusive Ressourcen
besitzen. Ein Plan erhält dadurch keine Action-Rechte.

## Ziel, Plan und bisherige Arbeit

Die originale Eingabeversion und die Root-Aufgabe bleiben identisch. Jeder
angenommene Vorschlag wird als eigenes unveränderliches Planartefakt
mit Revisionsindex, Vorgänger und Kontextbezug gespeichert.
Der Revisionsindex beginnt bei 0. Alte Pläne und Ergebnisse werden
erhalten. Die Relation `supersedes` verbindet die Planstände.
Der bisherige Vorschlagsvertrag ist `arcs.plan_candidate.v1`, sein
versioniertes Planartefakt `arcs.plan.v2`.

Neue Schritte erhalten einen Revisionsanteil in ihrer Route, etwa
`p1.r0.device` und `p1.r1.device`. Dadurch können neue Vorschläge weder alte
Aufträge noch Freigaben oder Reasoning-Ergebnisse durch Wiederverwendung eines
Schrittnamens übernehmen. Gewöhnliche Abhängigkeiten verbinden Schritte
derselben Generation. Ein ausdrücklich ausgewählter Export aus dem
direkten Vorgänger kann als zusätzliche Eingabe dienen.

## Übergang und Kontextgrenzen

Nach einem blockierten oder fehlgeschlagenen Schritt prüft die Runtime zuerst,
ob eine weitere Revision und deren Planungsbudget erlaubt sind. Der operative
Checkpoint hält die bisherigen Schrittzustände und erlaubten Ergebnisbezüge
fest. In derselben Journaltransaktion werden offene alte Zweige gestoppt.
Erst nach diesem Commit werden zugehörige laufende Antworten aufgegeben.

Das gemeinsame Artifact-Netz erhält den eingefrorenen Rückblick als
`arcs.plan_replanning.v1`. Der Planer bekommt die ursprüngliche Eingabe,
die frühere Vorschlagsstruktur, strukturierte Schrittzustände und ausgewählte
Ergebnisse. Die Auswahl verwendet ausschließlich die `export_fields` der
Betreibervorlagen und prüft die tatsächlichen abgeschlossenen Produzenten.
Rohe Adapterfehler und die vollständigen privaten Vorlagenparameter werden
nicht an den Planer weitergereicht.

Kontextplätze für Originaleingabe, Vorgängervorschlag und Rückblick sind
verpflichtend. Weitere freigegebene Ergebnisse werden in stabiler Reihenfolge
innerhalb der verbleibenden Plätze aufgenommen. `omitted_output_items`
kennzeichnet ausgelassene Ergebnisse. Abhängigkeiten und Memory behalten ihre
bestehenden Rechte und Grenzen; auch die normale Kontext-Bytegrenze gilt.
Nicht jeder im Rückblick genannte Versionsbezug ist damit bereits als
Artefaktinhalt an einen Reasoner übergeben worden.

## Fertige Teilergebnisse übernehmen

Die Eingangsroute setzt `target_schema_id` auf `arcs.plan_candidate.v2`
und verwendet einen Reasoner, der dieses Schema ausgibt. In
`plan.replanning` aktiviert `reuse_completed_exports: true` die Auswahl
von Vorgängerexporten. Die Einstellung gehört zur gespeicherten
Runtime-Definition. Die Ersatzplanung erhält in `task_replanning.previous_step_dependency` ein
Beispiel für die Form der Abhängigkeit. Ein neuer Schritt kann diese
Abhängigkeit so angeben.

```json
{
  "step_id": "bestand",
  "previous_revision": true,
  "payload_fields": ["value"]
}
```

`bestand` bezeichnet einen erfolgreich abgeschlossenen Schritt aus der
direkt vorherigen Generation derselben Aufgabe. Verfügbar sind
`export_fields`, deren Werte im eingefrorenen Neuplanungskontext enthalten
sind. Wurde ein Ergebnis des Schritts aus dem Kontextbudget ausgelassen,
kann der Planer den Schritt nicht als Quelle angeben. Für die neue
Aufgabe gelten die bestehenden Grenzen für Kontextplätze und Bytes.

Die Runtime speichert Ersatzpläne mit Vorläuferbindungen als
`arcs.plan.v3`. Ein reiner Vorläuferbezug ist bereits erfüllt. Bei gemischten Abhängigkeiten
verwendet der neue Schritt zuerst die festgehaltenen Vorläuferbindungen und
ergänzt sie, sobald die aktuellen Quellen fertig sind. Das gemeinsame
Artifact-Netz verknüpft den Ersatzplan mit den übernommenen Versionen.
Der alte Schritt wird dabei kein zweites Mal ausgeführt. Für neue Aktionen
gelten weiterhin deren eigene Freigaben und Ressourcenzuweisungen.

## Budgets, Ressourcen und Recovery

Bei aktivierter Neuplanung gelten `max_total_runtime_ms`,
`max_total_output_tokens` und `max_total_actions` über die gesamte Planfolge.
Die Rechnung umfasst auch die Planungsaufrufe einschließlich konfigurierter
Reasoner-Alternativen sowie alle bereits zugelassenen Schritte mit ihren
maximalen Goal-Runden und Assessments. Aufgegebene Schritte erstatten ihre
Zuteilung nicht. Ohne Neuplanung bleibt die bisherige Budgetsemantik erhalten.
Die Grenzen beschreiben konservative konfigurierte Höchstwerte, keine
nachträgliche Abrechnung tatsächlicher GPU-Zeit oder Geldkosten.

Für die Planfolge bleiben `1 + max_steps` Queueplätze reserviert. Aktive
Kindschritte werden darin genau einmal gezählt. Das hält Platz für die nächste
Expansion frei, selbst wenn neue Eingaben warten. Diese Queuegarantie ist
keine dauerhafte Belegung eines Reasoning-Workers. Die Root-Frist wird nicht
neu begonnen; Ersatzschritte erben weiterhin die frühere gemeinsame Frist.

Ein Ersatz-Goal erfasst neue Quellen und benötigt neue Verification sowie
seine eigene Freigabe, bevor es handelt. Ungeklärte Gerätewirkungen werden
nicht durch eine Planrevision aufgehoben. Die bestehenden exklusiven
Kontrollsitzungen und Klärungspflichten bleiben wirksam. Ressourcenidentitäten
müssen dieselbe reale Ressource installationsübergreifend benennen.

Wiederaufnahme verwendet denselben eingefrorenen Rückblick und dieselbe
Request-ID der jeweiligen Revision. Ein vor dem Queue-Commit veröffentlichtes
Planartefakt wird wiederverwendet. Ein Core- oder Journal-Schreibfehler wird
weitergegeben und erfindet keinen terminalen fachlichen Fehler für einen noch
nicht abgeschlossenen Checkpoint. Tatsächliche Adapterfehler behalten die
bestehende Fehlerisolation. Pause, Abbruch und Fristen gelten auch während
Neuplanung. Berichte zeigen `replanning`, die angenommenen Revisionen und die
zugehörigen alten und neuen Schritte.

## Kontingent für Reasoning-Anfragen

Setze optional `max_reasoning_requests` auf der Route, die eine Aufgabe
aufnimmt, zum Beispiel auf `12`. Jede Aufgabe erhält ein eigenes Kontingent.
Ein Arbeitsplan teilt es zwischen Planer, Teilschritten und allen
Ersatzgenerationen. Bei einer wiederkehrenden Prüfung erhält jeder Termin
ein eigenes Kontingent. Die Einstellung gehört zur gespeicherten Identität
der Runtime-Konfiguration und gilt somit auch nach einem Neustart.

Zum Kontingent zählen erstmals zugelassene Request-IDs. Dazu gehören auch
Fallbacks und Nachprüfungen. Ein einmal reservierter Platz bleibt verbraucht,
wenn eine Anfrage scheitert oder die Aufgabe sie später verwirft. Eine
Wiederaufnahme derselben Request-ID belegt denselben Platz. Vor einer
freigegebenen Aktion reserviert die Runtime den Platz für die Bewertung
ihrer Wirkung. Bei erschöpftem Kontingent werden keine weiteren
Reasoning-Anfragen zugelassen. Die Aufgabenwarteschlange zeigt unter
`reasoning_budget` die Aufgabensequenz sowie den Verbrauch und das Limit.
Im Kontrollpanel stehen dieselben Werte in den Aufgabendetails.

Bei einem Host mit `--monitor --control-stdin --stop-file monitor.stop`
fragt diese JSON-Zeile die ersten 25 Aufgaben einschließlich abgeschlossener
Einträge ab.

```json
{"request_id":"plan-budgets","control":{"kind":"queue","command":{"scope":"all","limit":25}}}
```

Ein Adapter kann nach einem verlorenen Antwortpaket dieselbe Anfrage erneut
erhalten. Je nach Anbieter kann dabei ein weiterer abrechenbarer Aufruf
anfallen. Das Kontingent begrenzt unterschiedliche ARCS-Request-IDs.

## Ausführbares Simulatorbeispiel

[Die Konfiguration](../config/plan-replanning/runtime.json) verwendet ausschließlich
die vorhandenen nativen Beispieladapter. Der externe Rules-Planer wählt zuerst
eine Goal-Vorlage mit einer Runde. Die Aufgabe verlangt zwei Zustandswechsel und
am Ende einen ausgeschalteten Lüfter. Nach dem ersten Einschalten ist das
Schrittbudget der ersten Vorlage erschöpft. Der Planer wählt im Revisionskontext
eine andere erlaubte Vorlage; deren frische Beobachtung führt zum Ausschalten.
Die Domänenregel liegt im externen Beispieladapter, nicht im Core.

```bash
cargo build --workspace --bins --examples --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/plan-replanning/host.json \
  --runtime apps/adapter-host/config/plan-replanning/runtime.json \
  --ticks 30
```

Die Journale liegen unter `var/plan-replanning/`. Der erste Lauf stoppt zur
manuellen Freigabe mit null Wirkungen. Für die konkrete angezeigte Goal-Sequenz
und Candidate-Version wird die normale [Goal-Review-CLI](goal-runtime.md)
verwendet. Nach der ersten Freigabe und weiteren Ticks erscheint eine neue
Goal-Sequenz mit eigener Candidate-Version. Erst deren Freigabe erlaubt die
zweite Wirkung. Ein wiederholter Start nach Abschluss erhält dieselben Belege.
Die Konfiguration ist ein deterministischer Simulatornachweis, kein LLM-Benchmark.

## Tests und offene Arbeiten

```bash
cargo test -p arcs-host --test plan_replanning --locked --offline -- --test-threads=1
```

Die Tests decken unter anderem Revisionen, Gesamtbudgets, volle Queue,
fehlgeschlagene Commitgrenzen, verlorene Antworten, private Felder, tatsächliche
Fallback-Produzenten, Pause, Fristen und zwei einzeln freigegebene
Simulatorwirkungen ab. Die vollständige allgemeine Abnahme bleibt offen:
freies Werkzeugargumentieren, selbst entwickelte Fähigkeiten, Neuplanung durch
beliebige externe Ereignisse, Rückgriffe auf Generationen vor dem direkten
Vorgänger und die Qualität allgemeiner Modellstrategien sind weitere Arbeiten.
Verteiltes Controller-Fencing und tatsächlicher Monatsbetrieb bleiben ebenfalls
eigene Abnahmen.
