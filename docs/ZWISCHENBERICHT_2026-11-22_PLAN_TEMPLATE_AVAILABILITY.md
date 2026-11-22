# ARCS-Zwischenbericht · 22. November 2026

## Verfügbarkeit von Planvorlagen

Mit `plan.include_template_availability` kann ein Planer an der bestehenden
Aufgabenroute die Verfügbarkeit der anfänglichen Reasoner-Optionen seiner
erlaubten Vorlagen einsehen. Der Host erfasst dazu den Sitzungsstatus,
bekannte Ausfälle und den Platz in den benannten Ressourcenpools.

Die Ausgangsplanung und jede Neuplanungsrevision erhalten einen eigenen Beleg
im gemeinsamen Artifact-Netz. Definition, Eingabe, Aufgabe und Revision
bestimmen seine Identität. Konkurrierende erste Beobachtungen verwenden den
zuerst gespeicherten Beleg. Der Planerrequest referenziert ihn als Kontext und
exportiert nur die Revision und die Liste der Vorlagen mit ihren Reasoner-Optionen.

Der Beleg dient als Entscheidungshilfe. Die tatsächliche Ausführung prüft
aktuelle Ressourcen, Grants und konkrete Binärdaten erneut. Die Option
verbraucht einen zusätzlichen Platz im Kontextbudget des Planers.

## Verifikation

Im Prozess-Test wählt der Planer zunächst die verfügbare Vorlage `a`. Nach
ihrem Ausfall wählt er aus dem neuen Beleg die Vorlage `b`. Während der
Neuplanung wird die erste Antwort unterbrochen. Nach Neustart bleiben der
Planerrequest und sein Beleg identisch. Die Ersatzvorlage läuft genau einmal.
Ein weiterer Test prüft die Ablehnung eines zu kleinen Kontextbudgets.

Bestanden sind 264 Core-Tests und 89 Host-Integrationstests aus den
Bereichen Capability-Suche, Planung, Neuplanung, Abbruch, Reasoner-Auswahl
und Aufgabenabhängigkeiten.
