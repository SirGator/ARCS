# Reasoning als allgemeiner Anschluss für Lösungsverfahren

Eine konkrete externe API-Anbindung liegt im Nachbarprojekt
[`llm-api`](../../llm-api/README.md). Sie unterstützt fünf konfigurierbare
API-Formate und wird vom Host ausschließlich per stdio/Reasoning V2 angesprochen.
Die [Beispielroute](../apps/adapter-host/examples/llm-tasks.md) verwendet getrennte
Planner-/Bewertungsinstanzen; Anbieterlogik und Prompts bleiben außerhalb von ARCS.

## Verbindliches Ziel

Reasoning ist der gemeinsame Zugang zu austauschbaren Lösungsverfahren.
Es muss mehrere mathematische, regelbasierte, LLM-basierte und andere
KI-Verfahren nebeneinander anschließen können. Eine Beschränkung auf ein
LLM oder einen einzigen Anbieter ist nicht vorgesehen.

| Verfahren | Mögliche Aufgaben |
| --- | --- |
| Mathematischer Solver | Optimierung, Gleichungen, Constraints, Pfadplanung |
| Regel- oder Suchverfahren | Entscheidungsregeln, symbolische Planung, Suche |
| LLM | Sprachverständnis, Vorschläge und Planung aus kuratiertem Kontext |
| Anderes KI-Modell | Klassifikation, Vorhersage oder gelernte Entscheidungen |
| Kombination | Teilprobleme mit verschiedenen Verfahren bearbeiten und Ergebnisse prüfen |

Die Rolle richtet sich nach der Aufgabe. Eine feste Datenumrechnung kann
weiterhin ein Transform sein. Derselbe mathematische Algorithmus kann als
Reasoning-Fähigkeit eingesetzt werden, wenn er für einen Auftrag unter
Constraints eine Lösung oder einen Plan ermittelt. Nicht jede KI-Anwendung
muss zwangsläufig den Reasoning-Port verwenden; feste Wahrnehmungsverträge
können zum Beispiel als Transform angeboten werden.

## Gemeinsamer Ablauf

Die Anwendung übergibt Ziel beziehungsweise Problem, ausdrücklich ausgewählte
Kontextartefakte, Constraints, gewünschten Ergebnisvertrag und Budgets.
Ein über seine `CapabilityRef` ausgewählter Adapter liefert nachvollziehbare
Lösungen, Pläne oder Entscheidungsvorschläge. Fachliche Problem- und
Ergebnisschemas bleiben bei den jeweiligen Komponenten.

Mehrere Adapter und mehrere Fähigkeiten eines Adapters sollen gleichzeitig
registrierbar sein. Zunächst wählt die Anwendung ihre Fähigkeiten ausdrücklich
aus. Sie kann Verfahren hintereinander einsetzen oder Ergebnisse vergleichen.
Automatische Auswahl, parallele Ausführung, Fallback-Ketten und gelerntes
Kostenrouting sind eigenständige spätere Ausbauschritte; sie entstehen nicht
allein durch einen gemeinsamen Port.

Ein Solver muss auch einen wohldefinierten Zustand wie „keine Lösung“ oder
„Budget ausgeschöpft“ berichten können. Solche fachlichen Ergebnisse sind von
Transportfehlern und unbegründeten Erfolgsmeldungen zu unterscheiden.

## Verfahren und Autorität getrennt halten

- Verfahrenstyp und Betreiber-Grant sind verschiedene Eigenschaften. Ein
  mathematischer Solver wird nicht allein durch seinen Namen zur vertrauten
  Systemkomponente.
- Die tatsächliche Herkunft bleibt erhalten. Modellbasierte Ergebnisse
  behalten die Modellherkunft und erhalten durch einen Solver- oder
  Kombinationsschritt nicht automatisch höheren Trust.
- Jeder Vorschlag wird gegen seinen Vertrag und seine zugelassenen
  Kontextreferenzen geprüft. Auch ein deterministischer Solver kann ein
  falsches Problem, falsche Annahmen oder eine fehlerhafte Implementierung haben.
- Reasoning besitzt keine unmittelbare Handlungsbefugnis. Für externe
  Wirkungen folgen weiterhin Verification, Approval und Action beziehungsweise
  der kontrollierte Output-Weg.
- Konkrete Solver, Modellanbieter und fachliche Regeln bleiben außerhalb
  des Core. Der Core enthält allgemeine Verträge und Kontrollmechanismen.

## Neutrale Ressourcen- und Herkunftsverträge

Gemeinsame Grenzen betreffen Kontextgröße, Ergebnisgröße, Anzahl der
Vorschläge und Laufzeit. Verfahrensabhängige Grenzen werden ausdrücklich
ergänzt: etwa Tokenlimits für LLMs oder Iterations-/Suchbudgets für Solver.
Ein mathematischer Solver soll keine erfundenen Tokenzahlen, Prompts oder
Sampling-Temperaturen benötigen.

Die Herkunftsspur muss Verfahren, Implementierungs-/Modellversion,
verwendete Eingaben und relevante Parameter nachvollziehbar machen.
LLM-Prompt und Temperatur gehören nur zu den Verfahren, für die diese
Angaben tatsächlich sinnvoll sind. Behauptete Spuren sind für sich kein
Nachweis fachlicher Korrektheit.

## Aktueller Implementierungsstand

Der bisherige Modellvertrag bleibt als V1 mit seinen Tests erhalten.
Daneben gibt es jetzt den neutralen V2-Vertrag im Protokoll, SDK, Core und
stdio-Host. Die Methoden `solver`, `rules`, `llm`, `learned` und `hybrid`
können über den gemeinsamen Port angeschlossen werden.

`GeneralReasoningService` und `Host::reason` validieren ausgewählten Kontext,
Zielschema, erlaubte Fähigkeiten, Ressourcen, Korrelation und Herkunft.
V2 verwendet `ReasoningResources` sowie `ReasoningTraceV2` mit Verfahren,
Implementierung, Version und passenden Ressourcenangaben. Solver und Regeln
brauchen ein Such-/Iterationsbudget; LLMs ein Tokenbudget. Modellbasierte
Methoden brauchen einen Model-Grant. Ein Model-Grant bleibt auch dann
konservativ Modellherkunft, wenn derselbe Prozess einen Solver anbietet.

Der Grant enthält dafür optional `reasoning_limits_v2`; alte Grants und die
V1-Artefaktschemas bleiben unverändert lesbar. Neue Audit-Artefakte verwenden
`arcs.reasoning_request.v2` und `arcs.reasoning_result.v2`. Requests und
Antworten werden dauerhaft dedupliziert; Kandidaten werden danach einzeln
idempotent gespeichert, ohne Verification, Approval oder Action zu erzeugen.
Neustarts können nach bereits gespeichertem Ergebnis fortsetzen. Aktuelle
Grants und Trust-Grenzen werden auch beim Replay geprüft.

Die Schema-Prüfung bestätigt die Form eines Vorschlags. Sie beweist weder
fachliche Korrektheit noch Optimalität. Die Ergebniszustände `solved`,
`no_solution` und `budget_exhausted` sind Aussagen des jeweiligen Backends.
Spuren und Verbrauchszähler werden vom Adapter berichtet. Token- und
Iterationsgrenzen muss das Backend tatsächlich durchsetzen; der Core prüft
zusätzlich die gemeldeten Zähler. Für stdio setzt der Host eine harte
Dialogfrist aus Runtime-Budget und Betreiber-Timeout. Ein direkter
Rust-Callback kann durch die synchrone Schnittstelle nicht präemptiv
unterbrochen werden; eine Überschreitung wird nach Rückkehr erkannt.

## Vorhandener Nachweis und nächste Schritte

[Das Input-/Reasoning-Beispiel](../apps/adapter-host/examples/reason-task.md)
verbindet drei unabhängig laufende Adapter: eine JSON-Eingangsquelle, einen
mathematischen Solver und einen ausdrücklich gekennzeichneten Modell-
Testadapter. Beide Reasoner verwenden denselben V2-Port. Ein SDK-Test weist
Input und Reason im selben Adapter nach. Die bestehenden Sensor-, Lüfter-
und Drohnenadapter verwenden weiterhin mehrere andere Rollen pro Prozess.

Der Modell-Testadapter liefert reproduzierbare Beispieldaten und führt
keine echte KI-Inferenz aus. Ein echter LLM-/ML-/NERVA-Anbieter ist noch
nicht implementiert. Ein solcher Adapter kann jetzt den V2-SDK-Callback
nutzen, ohne den Core auf seinen Anbieter zuzuschneiden. Die bestehende
Input-Rolle ist ebenfalls mit eigenem Polling und ACK über stdio verfügbar.

Die [Aufgaben-Runtime](../apps/adapter-host/examples/task-runtime.md) ordnet
Inputs und Beobachtungen jetzt dauerhaft konfigurierten Reasonern zu. Sie
speichert Aufgaben und Leseposition, setzt nach Neustarts fort und stellt
Ergebnisse zur fachlichen Prüfung bereit. Die Ausführung bleibt serielles Polling.
Optionale [Goal-Routen](../apps/adapter-host/examples/goal-runtime.md) koordinieren
anschließend Verification, Freigabe, Ausführung und frische Wirkungsdaten.
Ein separat konfigurierter Reasoner bewertet Wirkung und Ziel; ARCS speichert
den Outcome und plant innerhalb des Rundenbudgets weiter oder beendet die Aufgabe.
Ein echter Modelladapter, automatische Reasoner-Auswahl, Vergleich oder
Kombination mehrerer Ergebnisse und gelerntes Kostenrouting bleiben eigene Aufgaben.

## Explizite Binäreingaben

`Host::reason_with_data(request, selections)` ergänzt V2 um optional übertragene
Dateien, Bilder oder andere Binärdaten. Eine `InputDataSelection` nennt Name,
Quell-Capability, gespeicherte Quellversion und JSON-Pointer zur `DataReference`.
Diese Referenz muss im ausdrücklich ausgewählten Kontext enthalten sein und
exakt mit dem gespeicherten Payload übereinstimmen. Producer und Capability-Tag
des Core-Envelopes binden sie an die tatsächliche Quelle.

Die Quelle braucht `data.read` und `data_limits`. Der Empfänger braucht separat
`data.receive` und `input_data_limits` mit `max_total_bytes` und `max_objects`.
Pro Auftrag gelten höchstens acht Objekte und insgesamt 16 MiB; Betreibergrants
können engere Grenzen setzen. Der Core liest autorisierte Quellblöcke in einen
begrenzten Speicher und überträgt sie an genau eine Reasoning-Invocation.
Binärbytes erscheinen weder im kuratierten JSON-Kontext noch in Audit-Artefakten.
Der Empfänger darf sie in seinem eigenen Invocation-Journal aufbewahren.

Die Source-Vorbereitung benutzt die bestehenden Timeouts je Datenblock. Upload
und Berechnung beim stdio-Reasoner teilen danach eine gesamte Frist aus dem
Reasoning-Runtimebudget; jeder Austausch ist zusätzlich durch den Betreiber-
Timeout begrenzt. Ein Source-Timeout bleibt wiederholbar. Erfolgreiche Wiederholungen
prüfen aktuelle Rechte, übertragen und berechnen aber nicht erneut.
Quellen müssen referenzierte Bytes unveränderlich bereitstellen. Der Core
erzeugt durch den Transport keine zusätzliche fachliche Gewissheit oder Freigabe.

Der erste unterstützte Empfänger ist **Reasoning V2**. Transform hat noch keinen
entsprechenden Binäreingabe-Callback. Alte Reasoner ohne Binäreingaben verwenden
weiterhin denselben Dialog; das optionale Feld `input_data` wird bei leerer Liste
nicht serialisiert.

Der [externe Pixel-Suchnachweis](../../drone-sim/adapters/image-reasoner/README.md)
zeigt die Trennung: Die Kamera kennt keinen Suchauftrag, sondern bietet einen
Bildsnapshot an. Ein anderer Prozess bekommt Bildbytes und einen getrennten
Suchauftrag. Er erzeugt positive oder negative Bildbefunde mit Referenzen auf
beide Eingaben. Alle Bildformate, Suchregeln und Simulatorpakete bleiben außerhalb
des ARCS-Workspace. Ein Befundkandidat löst keine Bewegung aus.
