# Zwischenbericht vom 22. November 2026

Der Workspace-Adapter bereinigt beim Neustart die cgroup einer
abgebrochenen isolierten Git-Prüfung. Er reserviert den Namen im
vorhandenen SQLite-Journal und speichert Gerät und Inode der
Kindgruppe, bevor der Prüfbefehl starten kann. Hat der Adapter
das Prüfergebnis noch nicht gespeichert, erhält derselbe
freigegebene Aufruf `interrupted`.

Damit dient der bestehende Journal-Lock auch der Wiederaufnahme.
Die Konfiguration und das Ergebnisschema `workspace_revision_check.v3`
bleiben für installierte Ressourcenprofile gültig. Die Bereinigung
setzt voraus, dass der Adapter mit demselben Journal und einem
verfügbaren delegierten Elternbereich startet. Frühere Gruppen
ohne Journaleintrag bleiben unberührt.

## Nachweise

- Der Integrationstest beendete den Adapter per `SIGKILL`, während
  seine Prüfgruppe noch Prozesse enthielt. Der Neustart beendete
  den verbliebenen Prozessbaum, entfernte die Gruppe und meldete
  für dieselbe freigegebene Aktion `interrupted`. Eine benachbarte
  Gruppe ohne Journaleintrag blieb bestehen.
- Ein separater Test räumte eine leere Namensreservierung auf.
  Bei einer abweichenden Inode verweigerte der Adapter den Start.
  Die Testdatei für `cgroup.kill` und der Journaleintrag blieben
  erhalten.
- Die Kerneltests für CPU, Speicher, Prozesszahl und Deadline
  bestanden im delegierten systemd-Benutzerbereich. Der isolierte
  Ende-zu-Ende-Reparaturtest bestand dort ebenfalls.

[Profil und Testbefehle](../apps/adapter-host/examples/workspace-check.md#cpu--speicher--und-prozesslimits-setzen)
