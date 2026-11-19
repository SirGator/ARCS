# ARCS – Zwischenbericht zur echten Modellinferenz, 19. November 2026

Grundlage bleibt die unveränderte [ARCHITECTURE.md](ARCHITECTURE.md).
Dieser Slice ergänzt den [Gedächtnisbericht](ZWISCHENBERICHT_2026-11-18_EPISTEMIC.md)
um einen ausführbaren Inferenz- und Wiederaufnahmenachweis im vorhandenen
Goal-System. Modelllaufzeit, Gewichte und API-Übersetzung bleiben außerhalb
des Core in den vorhandenen Nachbarprojekten.

## Umsetzung

Die neue Konfiguration `apps/adapter-host/config/live-llm/` verbindet zwei
getrennte Reason-Prozesse mit einem lokalen Ollama-Modell. Planung und
Wirkungsbewertung besitzen eigene Profile, Grants und Journale. Sie teilen
einen benannten Rechenplatz; ihre zusätzliche Prozessumgebung enthält nur
Modellname und Endpunkt. Der konfigurierte Aktor ist der bekannte
Lüftersimulator. Die Betreiber-Notiz wird über den vorhandenen Input-Manager
in denselben Artifact Store aufgenommen und nur mit dem Feld `body` abgerufen.

Das [Beispiel `persistent-goal`](../apps/adapter-host/examples/persistent-goal.md)
prüft zwei aufeinanderfolgende Schritte: einschalten und wieder ausschalten.
Vor jeder Wirkung verlangt die normale Goal-Runtime eine eigene Freigabe.
Der Beispielrunner setzt ausschließlich ausdrücklich aktivierte Testfreigaben
für das bekannte Simulatorprogramm. Er vergleicht zudem den vorgeschlagenen
booleschen Schritt mit seinem Testszenario. Ein anderes Aktorprogramm wird
bereits vor dem Hoststart abgewiesen. Eine echte menschliche Einzelprüfung
wird damit nicht behauptet.

Der Runner beendet und öffnet den Host an mehreren Zustandsgrenzen. Ein
neunzig Tage späterer Scheduler-Termin wird simuliert. Die Aufgabe muss vor
diesem Termin ohne weitere Inferenz warten und danach mit frischen Quellen
und einer neuen Candidate-Version fortfahren. Eine Freigabe der alten Version
wird zurückgewiesen. Nach Abschluss werden Outcome, Ausführungsbezug und
Versuchsbelege bei einem weiteren Neustart erneut geprüft.

Der Nachweis liegt in der Anwendungsebene unter `examples/support/persistent_goal`.
Vorabprüfung, Session-Lebenszyklus und Prüfabfolge sind getrennt. Es wurden
keine Sprachinterpretation, Lüfter-Sonderfälle oder Modellbibliotheken in den
Core eingebaut. Die vorhandenen Manager, Registry, Grants, Artifact-Verträge,
Verification und Approval tragen den gesamten Ablauf.

## Tatsächlich beobachtete Modellfehler

Zwei erste Versuche mit dem vorhandenen `qwen2.5:7b` scheiterten nach der ersten
Simulatorwirkung. Im ersten Lauf bewertete das Modell die Einzelwirkung als
`unknown`, obwohl die frischen Daten den Zustandswechsel zeigten. Es vermischte
den noch offenen Gesamtauftrag mit der bereits erfolgreichen Einzelwirkung.
Die Runtime blockierte die Fortsetzung.

Darauf wurde der externe Assessor-Auftrag nach Zuständigkeit getrennt und
präzisiert: zunächst die Wirkung des aktuellen Schritts bewerten, anschließend
unabhängig davon den vollständigen ursprünglichen Auftrag. Die Core-Grenzen
wurden nicht gelockert. Im zweiten 7B-Versuch enthielt die Antwort eine
erfundene Evidenz-ID und wurde wiederum zurückgewiesen. Beide fehlgeschlagenen
Läufe und ihre ursprünglichen Journale bleiben lokal erhalten.

Für die im Architekturplan geforderte Modellgröße wurde anschließend
`qwen2.5:14b` in die vorhandene externe Modellumgebung geladen. Ollama prüfte
die SHA-256-Prüfsummen. Die lokale API meldet 14,8 Milliarden Parameter,
Quantisierung `Q4_K_M` und Modelldigest
`7cdf5a0187d5c58cc5d369b255592f7841d1c4696d45a8c8a9489440385b22f6`.
Die tatsächliche Inferenz erfolgt in dieser Umgebung auf der CPU.

Im ersten 14B-Durchlauf gelangen beide angeforderten Aktionen und die
Wiederaufnahme nach dem simulierten Termin. Die letzte Antwort wurde wegen
ungültiger oder doppelter Kontextverweise abgewiesen. Darauf wurde im externen
Adapter eine ausdrückliche Referenzpolicy ergänzt: Wiederholte Nennungen
derselben zulässigen ID können als eindeutige Liste kanonisiert werden.
Die Prüfung sämtlicher IDs gegen den gelieferten Kontext erfolgt davor.
Payloads, fachliche Aussagen, Evidenzlisten und Berechtigungen werden nicht
korrigiert oder erweitert. Ohne diese Policy bleibt der strikte Vertrag
bestehen; ein Policywechsel verlangt wegen der Journalbindung neue Journale.

Der nächste 14B-Durchlauf zeigte im Trace eine entfernte doppelte Nennung.
Beide beobachteten Wirkungen wurden als erfolgreich bewertet. Die abschließende
Antwort bezeichnete die gesamte Aufgabe im Detail als erledigt, lieferte aber
`goal_status: continue`; der Core setzte daher `budget_exhausted` und der
Nachweis blieb fehlgeschlagen. Der externe Assessor-Auftrag wurde anschließend
um eine kurze explizite Abschlusszusammenfassung und die Übereinstimmung des
kategorischen Status mit dieser Zusammenfassung präzisiert. Alle bisherigen
Fehlversuche bleiben erhalten.

Ein weiterer 14B-Versuch mit dieser präzisierten Anweisung scheiterte nach der
ersten Wirkung: Die Antwort nannte als Evidenz eine frühere Assessment-Version,
deren ID zwar in den Constraints stand, deren Artefaktinhalt aber nicht im
aktuellen Kontext vorlag. Der Core wies diese Referenz zurück. Damit sind
insgesamt fünf fehlgeschlagene Läufe erhalten: zwei mit 7B und drei mit 14B.

Die abschließende externe Adapterkorrektur trennt Transportprovenienz von
fachlicher Bewertung. Mit der ausdrücklichen Policy `reference_policy: context`
setzt der Adapter die übergebenen Kontextversionen als Eingangsreferenzen.
Das ist keine Behauptung, das Modell habe jede Quelle inhaltlich bestätigt.
Für den Assessor wird ausschließlich das Metadatenfeld `evidence_version_ids`
aus konfigurierten Before-/After-Verweisen gebunden. Jede ID muss bereits im
Kontext enthalten sein; Kardinalität und erlaubte Werte werden geprüft.
Das Modell erhält ein um diese gebundenen Metadatenfelder reduziertes
Antwortschema. Fachliche Felder wie `effect`, `goal_status`, Detailbewertung
und Aktionsvorschläge stammen weiterhin unverändert vom Modell. Der Trace
kennzeichnet die Bindung; der Core behält seine regulären Prüfungen.

Die Adapteränderungen sind im Nachbarprojekt als `4dcf7c4` und `a2475cf` lokal committet.
Dieses Repository besitzt keinen Remote. Der vollständige
[Patch](../apps/adapter-host/examples/patches/llm-api-reference-policy.patch)
ist deshalb zusätzlich Bestandteil dieses ARCS-Zwischenstands und kann auf
dem bisherigen Adapterstand `3c7eca3` geprüft und angewendet werden.

## Erfolgreicher realer 14B-Lauf

Mit der ausdrücklichen Referenzbindung besteht der vollständige Nachweis:

| Messung | Beobachtung |
| --- | --- |
| Modell | Qwen2.5, 14,8B Parameter, `Q4_K_M`, lokale CPU-Inferenz |
| Tatsächliche Laufzeit | 192.488 ms, rund 192 Sekunden |
| Modellaufrufe | Zwei Planungen und vier Ziel-/Wirkungsbewertungen |
| Ausgabetokens je Antwort | 59, 60, 90, 74, 80, 109 |
| Testfreigaben / Simulatorwirkungen | Genau zwei / genau zwei |
| Host-Wiederöffnungen | Vier, bei unveränderten abgeschlossenen Ausführungs-/Outcome-Versionen |
| Zeitlicher Abstand | 90 Tage simuliert; vor Fälligkeit keine neue Inferenz |
| Ergebnis | Alle sieben Checkpoints bestanden; Core-Outcome `success` |

Die unabhängige Journalprüfung bestätigt sechs abgeschlossene API-Aufträge
und sechs gültige Reasoning-Belege. Jeder Auftrag enthält die ursprüngliche
Aufgabe und genau das freigegebene Notizfeld `body`. Der private Marker fehlt
in sämtlichen tatsächlich gespeicherten SDK-Aufträgen. Zwei Approval-Artefakte
und der abschließende Simulatorzustand `enabled: false`, `actuation_count: 2`
stimmen mit dem Ablauf überein. Vorher und nachher meldet Ollama denselben
14B-Modelldigest. Das temporäre Testbackend verwendet einen eigenen lokalen
Port; die heruntergeladenen Gewichte bleiben im externen Modellprojekt.

Die sieben gespeicherten Checkpoints sind `first_review`,
`first_review_reopened`, `waiting`, `waiting_reopened_before_due`,
`second_review`, `completed` und `completed_reopened`. Damit ist die konkrete
v2-Anforderung RSN-01 erfüllt. Die Gesamtarchitektur bleibt in Entwicklung.

## Regression und Nachweisumfang

Fünf neue Prozess-Tests führen denselben Runner mit deterministischen externen
Rules-Reasonern aus. Sie prüfen den vollständigen Ablauf mit genau zwei
Wirkungen, die Aufbewahrung vorhandener Journale, `no_solution`, einen
fachlich anderen Handlungsvorschlag, unzulässige Freigabepolicy und ein fremdes
Aktorprogramm. Diese Regression wird ausdrücklich nicht als echte
Modellinferenz ausgegeben.

Die Qualifikation erwartet sechs echte beziehungsweise im Regressionstest
deterministische Reasoning-Antworten: pro Runde initialer Check, Planung und
Wirkungsbewertung. Jeder Auftrag muss die ursprüngliche Aufgabe und das
freigegebene Notizfeld enthalten. Das private Notizfeld darf nicht im
Modellkontext erscheinen. Auch eine formal gültige Antwort wird nicht mit
fachlicher Qualität gleichgesetzt: Ein nicht abgeschlossener oder unzulässiger
Ablauf liefert `passed: false`.

Die abschließenden Prüfungen ergeben:

| Prüfung | Ergebnis |
| --- | --- |
| Rust-Workspace, getrenntes Build-Verzeichnis, sequenzielle Testausführung | 595 bestanden, 0 fehlgeschlagen |
| Neue Qualifikationstests mit echten Adapterprozessen | 5 bestanden, im Workspace-Ergebnis enthalten |
| Externer LLM-Adapter einschließlich Referenzbindung und Provider-Fixtures | 28 bestanden, 0 fehlgeschlagen |
| Workspace-Build einschließlich aller Binärprogramme und Beispiele | Bestanden |
| Formatprüfung und Clippy über alle Targets | Bestanden; zwei bestehende Clippy-Hinweise in `reasoning/service.rs` |
| SDK-Patch vorwärts gegen sauberen Basisstand und rückwärts gegen aktuellen Stand | Bestanden; reproduziert alle vier geänderten Dateien |

Die lokalen Prüfprotokolle liegen unter `var/verification-live-llm/`.
Die Modelljournale liegen getrennt unter `var/live-llm/`; die beiden 7B-
Fehlversuche verwenden andere Datenbestände als der 14B-Nachweis.

## Grenzen

Die simulierten neunzig Tage sind keine gemessenen neunzig Tage Betriebszeit.
Die tatsächlichen Host-Unterbrechungen sind kurz. Modell-Timeouts bleiben an
die reale Zeit gebunden. Der Beispielrunner genehmigt keine physische Drohne
und ersetzt keine Freigabeoberfläche für reale Betreiber.

Ein einzelner geordneter Simulatorfall ist kein allgemeiner Modellbenchmark.
Die zwei 7B-Versuche und vier 14B-Versuche mit geänderten Profilen sind keine
statistische Zuverlässigkeitsmessung. Der präzisierte Assessor-Auftrag ist
eine geänderte Konfiguration, kein verborgenes Umschreiben einer Modellantwort.
Der vorhandene Modelldienst berichtet Namen und Digest; die einzelnen
Inference-Antworten tragen keine kryptographische Gewichtsattestation.

Allgemeine unbekannte Ziele, Neuplanung nach geänderten Voraussetzungen,
autonome Drohnensuche, Werkzeugentwicklung, verteiltes Controller-Fencing
und langfristiger realer Betrieb bleiben weitere Architekturabnahmen.
