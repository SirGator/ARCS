# ARCS – Reasoning im Hintergrund, 20. November 2026

Die Aufgabenruntime verwendet den vorhandenen Reasoning-Pool mit jedem
konfigurierten Limit. Standardmäßig läuft höchstens ein Modellaufruf.
Währenddessen bearbeitet der Host weitere Eingaben, Beobachtungen,
Steuerbefehle und ausführbare Schritte anderer Manager. Der Core bereitet
die Invocation vor und validiert und speichert die Antwort.
[Parallelitätsvertrag und Ausführung](../apps/adapter-host/examples/parallel-runtime.md)

Fehlgeschlagene Aufträge geben Queuekapazität frei. Änderungen am
Worker-Limit greifen nur in die Ausführungsplanung ein. Beim Öffnen eines
älteren Journals übernimmt ARCS dessen bereits gespeicherten Fehlerstatus
auch in den Goal-Checkpoint. Neuplanung bewahrt fehlgeschlagene Schritte
als Fehler und beendet noch offene Schritte des alten Plans. Autorisierte
Reasoner-Alternativen verwenden ihre Recovery-Policy. Die direkten
Host-Aufrufe `reason` und `reason_with_data` bleiben synchrone Einzelaufrufe.

Der Monitor-Test wartet auf einen blockierten Test-Reasoner, fragt die
Warteschlange ab und erhält eine spätere Sensorbeobachtung. Eine zweite
Reasoning-Aufgabe wartet bis zur Freigabe des einzigen Worker-Platzes.
Wiederanlauftests unterscheiden zwei Fälle. Bei einem bereits gespeicherten
Modellfehler wird der Auftrag ohne weiteren Modellaufruf beendet. Nach einem
unterbrochenen Ergebnis-Commit bleibt die ursprüngliche Invocation für
denselben Adapter wiederaufnehmbar.

Das GTK-Kontrollpanel bestand 46 Tests und der separate Drohnensimulator 33.
Ein Worker-Test erreichte im breiten Lauf das Zeitlimit seines Testsignals.
Mit einem größeren Zeitbudget für dieses Signal bestanden anschließend
alle neun Tests zu parallelem Reasoning, einschließlich der separaten
Timeout-Prüfung. Die Prüfprotokolle liegen unter `var/verification-reasoning-default/`.

Die Abnahme im monatelangen Dauerbetrieb und an realen Controllern steht aus.
