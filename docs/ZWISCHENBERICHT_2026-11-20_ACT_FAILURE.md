# ARCS – Fehlerisolierung bei Geräteaktionen, 20. November 2026

Ein gespeicherter Act-Fehler beendet die betroffene Aufgabe auch mit den
Standardwerten für Reasoning und Request. Der Scheduler kann dadurch weitere
Aufgaben annehmen und ausführen. Fehlertext, Queue-Status und Goal-Checkpoint
werden in einer Journal-Transaktion aktualisiert. Ungeklärte
Gerätereservierungen bleiben in der Reservierungsprüfung sichtbar.
[Architekturspezifikation, PAR-08/PAR-09/PAR-10](ARCHITECTURE.md)

Bei einem Neustart liest die Goal-Runtime den Zustand der Invocation aus dem
Core. Einen bereits gespeicherten Fehler kann sie bei einem fehlenden
Aufgaben-Checkpoint nachtragen. Ein Dispatch ohne verbuchtes Ergebnis
bleibt mit seiner bisherigen Invocation-ID wiederaufnehmbar. Dies gilt
auch bei einer fehlgeschlagenen Veröffentlichung eines Ausführungsbelegs.
[Goal-Runtime](../apps/adapter-host/examples/goal-runtime.md#persistenz-betrieb-und-grenzen)

Die Prozesstests lassen den Lüftersimulator eine Aktion speichern und
liefern anschließend einen temporären Fehler an den Host. Unabhängige
Aufgaben erreichen nach diesem Fehler und nach einem Neustart die Prüfung.
Ein weiterer Test blockiert die Aktualisierung des Aufgabenjournals per
SQLite-Trigger. Der Neustart trägt den Fehler aus dem Core nach, ohne den
Aufruf erneut an den Simulator zu senden. Ein Core-Test prüft die
Wiederaufnahme nach einem unterbrochenen Commit des Ausführungsbelegs.

Workspace-Build und Formatprüfung waren erfolgreich. Clippy meldete
zwei bereits vorhandene Warnungen im Reasoning-Service. Die lokalen
Prüfprotokolle liegen unter `var/verification-act-failure/`.

Die Abnahme an realen Controllern und im monatelangen Dauerbetrieb steht aus.
