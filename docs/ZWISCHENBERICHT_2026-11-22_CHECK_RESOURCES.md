# Zwischenbericht vom 22. November 2026

Isolierte Git-Prüfungen können jetzt CPU-Zeit, Speicher und die
Anzahl der Tasks im gesamten Prozessbaum begrenzen. Der Adapter
legt pro Auftrag eine Kindgruppe in einem vom Betreiber delegierten
cgroup-v2-Bereich an und setzt die Limits vor dem Start von
Bubblewrap. Ein fehlender Controller verhindert den Prüfstart.

Das Ausführungsartefakt hält die eingestellten Grenzen und die
Kernelzähler fest. Ein Speichermangel oder die abgewiesene
Erzeugung weiterer Tasks führt zu `resource_limit`. Eine bloße
CPU-Drosselung ist am Zähler erkennbar. Damit kann eine bestandene
Prüfung weiterhin als Erfolg eingehen.

## Nachweise

- Der Kerneltest begrenzte eine CPU-lastige Prüfung auf 25 Prozent
  eines logischen Kerns, dokumentierte Drosselung und meldete die
  Prüfung als bestanden.
- Ein Prüfprogramm überschritt 64 MiB. Die Kernelquittung enthielt
  einen OOM-Kill. Bei der separaten Taskprüfung wurde eine weitere
  Prozesserzeugung am Limit abgewiesen. Beide Quittungen meldeten
  `resource_limit`. Ein weiterer Test prüfte das Zeitlimit mit einem
  Kindprozess. Die Prüfgruppen wurden nach jedem Lauf entfernt.
- Der Reparaturplan verwendete den neuen Belegvertrag für
  Erstprüfung und Folgeprüfung. Nach einem Neustart und eigenen
  Testfreigaben bestand der neue Git-Commit.
- Ein künstlicher Bereich ohne Kind-Controller startete den
  Prüfbefehl nicht und lieferte `runner_error`.

Für die Tests wurde ein temporärer systemd-Benutzerbereich
delegiert. Der Adapter verwaltet dort seine Kindgruppen. Einen
abrupten Adapterabbruch überlebt die Deadline des Prüfprozesses,
wobei eine leere Gruppe zur späteren Bereinigung zurückbleiben kann.

[Profil und Testbefehle](../apps/adapter-host/examples/workspace-check.md#cpu--speicher--und-prozesslimits-setzen)
