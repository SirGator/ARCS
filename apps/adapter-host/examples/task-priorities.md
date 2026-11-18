# Dauerhafte Prioritäten und faire Aufgabenverteilung

Diese Erweiterung setzt einen Teil von `ARCHITECTURE.md`, v3 §7.2
PAR-05/PAR-10/PAR-12 um. Die vorhandene Runtime verteilt begrenzte
Aufgabenschritte anhand von Betreiberprioritäten. Sie benötigt keine zusätzliche
Steuerungsinstanz und keine wartenden Threads pro Aufgabe.

## Konfiguration und Beispiel

Jede feste Reasoning-, Plan- oder Goal-Route und jede freigegebene Schrittvorlage
kann `"priority": 0`, `1`, `2` oder `3` angeben. Höhere Zahlen erhalten mehr
Ausführungsversuche. Ohne Angabe gilt `0`; dieses Feld bleibt dann auch in der
gespeicherten Definition unsichtbar. Bestehende Core-Definitionen, Journale und
Planartefakte bleiben kompatibel. Eine spätere Änderung der Routenpriorität
erfordert wie andere semantische Änderungen eine neue Runtime-ID und ein neues
Journal.

Das vollständige Beispiel verwendet zwei externe deterministische Reasoner,
einen gemeinsamen Kapazitätspool und getrennte Daten unter `var/priorities/`:

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/priorities/host.json \
  --runtime apps/adapter-host/config/priorities/runtime.json \
  --ticks 20
```

`nearest-model` erhält Priorität 3, `nearest-solver` die Standardpriorität 0.
Pro Tick wird höchstens eine Aufgabe ausgewählt. Beide Aufgaben enden als
`ready_for_review`; es entstehen keine Handlungen. Ein weiterer Start übernimmt
die vorhandenen Ergebnisse und Zähler.

Ein Modell darf nur freigegebene Vorlagen auswählen. Es kann seine Priorität
nicht in einem Planvorschlag setzen. Dynamische Schritte übernehmen die
Priorität ihrer konkreten Betreiber-Vorlage; die Priorität des Planungsauftrags
wird nicht automatisch an alle Schritte vererbt.

## Verteilung und Wiederaufnahme

Die Klassen teilen sich einen wiederholten Umlauf mit 15 Plätzen:

| Priorität | Gewicht | Plätze bei durchgehend auswählbaren Klassen |
| --- | --- | --- |
| 0 | 1 | 1 von 15 |
| 1 | 2 | 2 von 15 |
| 2 | 4 | 4 von 15 |
| 3 | 8 | 8 von 15 |

Die Plätze sind ineinander verschachtelt. Leere Klassen werden sofort
übersprungen. Eine durchgehend auswählbare Klasse erhält deshalb spätestens
nach 15 erfolgreichen Auswahlen wieder einen Schritt. Innerhalb jeder Klasse
wechseln sich die Aufgaben nach ihrer Sequence ab. Der Umlauf friert die
Obergrenze seiner Aufgabengruppe ein: Neue Eingaben können laufende ältere
Aufgaben nicht durch immer höhere Sequences dauerhaft vom nächsten Turn
verdrängen. Sie nehmen spätestens im folgenden Klassenumlauf teil.

Jede Aufgabe wird höchstens einmal pro Tick gewählt. Bei wenigen Aufgaben
oder kleinen Tick-Grenzen ergibt sich daher nicht zwingend das Verhältnis
1:2:4:8. Die Gewichte beschreiben die Klassenverteilung bei ausreichend vielen
auswählbaren Aufgaben. Auf Termine, Ereignisse oder Abhängigkeiten wartende
Aufgaben verbrauchen keinen Schritt. Ihre Aktivierung verwendet weiterhin die
vorhandenen begrenzten Scheduler-Abfragen. Auf menschliche Freigabe wartende
Goals erhalten dagegen weiterhin Prüfschritte: Nach einem Absturz kann die
Entscheidung bereits im Core vorliegen, während das Journal noch wartet.
Diese Prüfung startet keinen Reasoner und erteilt keine neue Freigabe.

Umlaufplatz, Klassenzeiger, Gruppengrenze und Auswahlzähler liegen im
bestehenden operativen SQLite-Journal. Ihre Änderung und die Besuchsmarkierung
des aktuellen Ticks werden gemeinsam committed, bevor Core-Zugriffe oder
externe I/O beginnen. Ein Absturz kann einen Turn verbrauchen, ohne dass der
Schritt begonnen hat. Die Wiederaufnahme verwendet weiterhin die vorhandenen
Invocation-IDs und die Deduplizierung der Adapter. Ein verlorener Scheduler-Turn
autorisiert keine zusätzliche Handlung.

Die additive Migration ordnet vorhandenen Aufgaben ihre Priorität aus der
gebundenen Betreiberdefinition zu, einschließlich bereits erzeugter Schritte.
Sie erzeugt keine neuen Core-Ergebnisartefakte. Der erste Start eines älteren
Journals beginnt die Auswahlzähler bei null.

## Berichte und Grenzen

Der CLI-Bericht enthält `dispatch`, Monitor-Snapshots enthalten
`runtime.dispatch`:

- `policy`: `weighted_round_robin_v1`.
- `next_slot`: nächster Umlaufplatz, 0 bis 14.
- `classes`: pro Priorität das Gewicht, `selections`, `last_sequence` und
  `round_end`. Auswahlzähler bleiben über Neustarts erhalten und sättigen beim
  größten SQLite-Integer.

Gezählt werden Auswahlen, einschließlich erfolgloser Versuche wegen belegter
Ressourcen und Abfragen noch laufender Reasoner. Das sind keine Erfolgs-,
Token-, CPU-Zeit- oder Latenzmetriken. Die Fairness gilt innerhalb eines
Runtime-Journals. Mehrere Runtimes erhalten dadurch noch keine gemeinsame
gewichtete Kapazitätsverteilung.

Priorität kann weder einen laufenden synchronen Adapteraufruf unterbrechen
noch eine belegte Geräte-Reservierung übernehmen. Die bestehenden Grants,
Verification, Approval und Ressourcenprüfungen gelten bei jedem Versuch.
Freie Ausführungskapazität und tatsächlicher Aufgabenfortschritt werden mit
dieser Änderung nicht garantiert. Ressourcenfairness zwischen mehreren Hosts,
Preemption, dynamische Prioritätsänderungen und umfassende Durchsatz- und
Latenzmessungen bleiben offen.

## Prüfung

```bash
cargo test -p arcs-host --lib task_runtime::journal::dispatch --locked --offline
cargo test -p arcs-host --test task_priorities --locked --offline
```

Vier Scheduler-Tests prüfen Gewichte, Klassen- und Aufgabenfortschritt bei
fortlaufenden Eingaben, ausgesparte Wartezustände, einmalige Tick-Besuche und
atomaren Rollback. Fünf Prozess-Tests prüfen wiederholte Einzelschritt-Neustarts,
einen vollständigen Host-Neustart, Migration dynamischer Schritte, unveränderte
Standardidentität und die Ablehnung einer Prioritätsvergabe durch das Modell.
