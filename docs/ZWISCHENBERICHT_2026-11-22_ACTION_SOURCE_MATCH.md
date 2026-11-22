# Zwischenbericht vom 22. November 2026

Ein Betreiber kann Felder eines Aktionskandidaten an die
gespeicherten Vorher-Quellen eines Goals binden. Der Host prüft,
ob der Kandidatenwert mit dem festgehaltenen Quellenwert exakt
übereinstimmt. Ein Vergleich darf nur ausdrücklich ausgewählte
Felder der Quelle verwenden. Der Verification-Bericht benennt
Feld und Quelle bei Abweichungen.

Im Reparaturablauf vergleicht der Host den vorgeschlagenen
Dateipfad und Basis-Commit mit dem Quelltextartefakt. Die Prüfung
läuft vor der Freigabe und erneut vor dem Versand der Action.

## Nachweise

- Ein formal valider Vorschlag mit einem älteren Basis-Commit
  wird vor der Freigabe blockiert. Der Vorgang schreibt keinen
  zweiten Reparatur-Commit.
- Ein Vorschlag für eine andere, ebenfalls zum Editieren
  freigegebene Datei scheitert am Pfadvergleich. Ein Vergleich
  auf ein nicht exportiertes Quellenfeld wird schon beim Laden
  der Runtime-Konfiguration abgewiesen.
- Das lokale Modell `qwen2.5:14b` erzeugte im isolierten
  Rechnerprojekt einen passenden Vorschlag mit 117 Ausgabetokens.
  Beide Vergleiche bestanden. Nach getrennten Testfreigaben
  für Änderung und Prüfung bestand der neue Commit.

Die Tests belegen die Bindung an die Quelle und den Ablauf für
diesen Python-Fehlerfall. Die fachliche Richtigkeit der Änderung
wird weiterhin über die Prüfung des neuen Commits bewertet.

[Konfiguration des Quellabgleichs](../apps/adapter-host/examples/goal-runtime.md#aktionsfelder-an-die-erfasste-quelle-binden)
