# Zwischenbericht vom 21. November 2026

ARCS kann über `check.revision` eine freigegebene Softwareprüfung für
einen bestimmten Git-Commit ausführen. Der Betreiber wählt die
verfügbaren Programme und die lokale ARCS-Referenz. Der Auftrag bindet
den Namen der Prüfung und den vollständigen Commit-Hash. Der Adapter
bereitet die temporäre Kopie aus Git-Objekten vor, speichert die
Startabsicht und übergibt die Quittung mit Revision und Prüfstatus als
Artefakt an den Core. Vorhandene `check.run`- und `edit.apply`-Profile
bleiben verwendbar.

## Nachweise

- Die alte Revision scheitert am echten Rechner-Test und die
  freigegebene Korrektur besteht ihn. Beide Prüfungen laufen nach
  Veröffentlichung der Korrektur. Die ursprüngliche Projektdatei
  bleibt fehlerhaft, eine unversionierte Datei bleibt außerhalb
  der Prüfkopie und Git-Exportattribute verändern die Kopie nicht.
- Ein Commit außerhalb der ARCS-Referenz, ein Blob-Hash, ein Baum
  mit Symlink und ein nicht freigegebener Prüfprogrammname werden
  vor dem Prüfstart abgewiesen.
- Verliert der Core die Revisionsergebnisquittung, übernimmt er
  sie nach einem Neustart aus dem Adapterjournal. Der Test weist
  einen einzigen Prozessstart nach, obwohl die Arbeitsreferenz
  vor der Wiederaufnahme zurückgesetzt wurde.

Die Integrationstests geben Edit und Prüfung als eigene Actions
frei. Die automatische Übergabe der Edit-Quittung an einen abhängigen
Prüfschritt desselben Plans steht noch aus.

[Adapteranleitung mit Profil, Grant und Schema-Verträgen](../apps/adapter-host/examples/workspace-check.md)
