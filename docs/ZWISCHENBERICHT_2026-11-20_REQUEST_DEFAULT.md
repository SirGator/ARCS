# ARCS – Nicht blockierende Standardabfragen, 20. November 2026

Alle Quellenabfragen der Goal-Runtime verwenden den begrenzten Request-Pool.
Mit dem Standardwert eins wartet die Aufgabe auf ihre Abfrage, während der
Scheduler unabhängige Aufgaben und Steuerbefehle bearbeitet. Der Host
bereitet die Invocation im Core vor. Der Worker überträgt die Anfrage und
liefert die Antwort zur Prüfung und Speicherung an den Host zurück.
[Architekturspezifikation, PAR-01/PAR-02/PAR-08/PAR-10/PAR-11](ARCHITECTURE.md)

Ein gespeicherter Quellenfehler beendet die betroffene Aufgabe. Nach einem
unterbrochenen Journal-Commit übernimmt der Wiederanlauf den Fehler aus dem
Core. Scheitert die atomare Veröffentlichung einer validierten Antwort,
bleibt der Dispatch für die Wiederaufnahme mit derselben Invocation-ID offen.
[Quellenabfragen](../apps/adapter-host/examples/parallel-requests.md)

Im Monitor-Test wird die Antwort einer Quelle zurückgehalten. Der Monitor
beantwortet eine Abfrage der Warteschlange. Eine unabhängige Aufgabe erreicht
die Prüfung. Nach dem simulierten Quellenfehler nimmt die Runtime einen
weiteren Auftrag an. Weitere Tests unterbrechen getrennt den Fehler-Commit
im Aufgabenjournal und den Ergebnis-Commit im Core und prüfen den jeweiligen
Wiederanlauf.

Das Kontrollpanel aktualisiert eine leere erste Warteschlangenseite,
sobald der Host später hinzugekommene Aufgaben meldet. Beim Blättern bleibt
die gewählte Seitengrenze erhalten. Der GTK-Test öffnet die leere
Warteschlange vor dem ersten Input und prüft anschließend beide Seiten.

Workspace-Build und Formatprüfung waren erfolgreich. Die Prüfprotokolle
liegen unter `var/verification-request-default/`.

Die Abnahme an realen Controllern und im monatelangen Dauerbetrieb steht aus.
