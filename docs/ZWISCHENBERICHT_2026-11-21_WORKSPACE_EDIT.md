# Zwischenbericht vom 21. November 2026

ARCS kann freigegebene Codekorrekturen als lokale Git-Version speichern.
Die optionale Fähigkeit `edit.apply` verwendet den vorhandenen
Act-Manager. Der Betreiber legt eine lokale Referenz und die erlaubten
Dateien fest. Der freizugebende Kandidat bindet den vollständigen
Basis-Commit, den Dateipfad und den neuen Text.
[Konfiguration des Workspace-Adapters](../apps/adapter-host/examples/workspace-check.md#freigegebene-codekorrekturen-als-git-versionen-speichern)

Der Adapter erzeugt die neue Git-Version in einem separaten Index.
Bei der Veröffentlichung vergleicht Git den aktuellen Referenzstand
atomar mit der freigegebenen Basis. Die Quittung meldet den angewendeten
Commit, einen identischen Stand oder einen Konflikt. Das Journal
speichert vorbereiteten Commit und Zeitstempel vor der Veröffentlichung.
Damit kann der Adapter bei der Wiederaufnahme einen bereits
veröffentlichten Commit übernehmen oder ein noch unveröffentlichtes
Git-Objekt mit derselben ID rekonstruieren.

## Geprüfte Abläufe

- Ein echter Softwaretest scheitert am ursprünglichen Rechner. Die
  freigegebene Änderung korrigiert die Addition. Nach einem unterbrochenen
  Quittungs-Commit verwendet ARCS dieselbe neue Git-Version. Ein
  konfiguriertes Testprogramm liest den veröffentlichten Stand und
  besteht.
- Ein konkurrierender lokaler Git-Vorgang behält seinen Commit. Ein
  Kandidat auf einer früheren Basis liefert einen Konflikt.
- Der Test entfernt ein unveröffentlichtes Git-Objekt aus einem
  temporären Repository. Der Wiederanlauf rekonstruiert dieselbe
  Commit-ID.
- Die Dateigrenze schützt den konfigurierten Umfang. Der Adapter weist
  eine Datei außerhalb der Freigabeliste und einen Symlink ab.

Acht Adaptertests und 16 Host-Regressionstests bestanden. Ein früheres
Prüfjournal wurde mit seinem gespeicherten Beleg erneut geöffnet.
Das dokumentierte Edit-Profil meldete die Fähigkeit `edit.apply`
beim Start.

Der Integrationstest koordiniert die separat freigegebenen Prüf- und
Edit-Aufrufe. Ein Plan, der die korrigierte Commit-ID selbst an den
Prüfadapter weitergibt, gehört zur weiteren Umsetzung von TEST-02.
