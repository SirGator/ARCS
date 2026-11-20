# ARCS – Geprüfte Aufgabenausgabe, 21. November 2026

Ein Betreiber kann ein bestimmtes Ergebnis einer fertigen Aufgabe
ausgeben. Der Bericht übernimmt dessen Vertrauensniveau und ist mit
der Ergebnisversion verknüpft. Die Vorschau bindet den geprüften Bericht
an den gewählten Zieladapter. Eine Zustimmung oder Ablehnung enthält
einen Betreiber und eine Begründung.
[Aufgabenausgabe](../apps/adapter-host/examples/output-delivery-queue.md)

Die Runtime speichert die Entscheidung vor der Freigabe im Core. Ein
Prozesstest unterbricht danach die Aufnahme in die Ausgabewarteschlange.
Beim Neustart übernimmt ARCS die vorhandene Freigabe und stellt den Bericht
einmal zu. Der Test prüft auch eine fremde Ergebnisversion, eine nicht
passende Verification und eine Ablehnung.

Im GTK-Kontrollpanel ist die Ausgabe über die Aufgabenwarteschlange
erreichbar. Ein Wechsel des Zieladapters erfordert eine neue Vorschau.
Die Quittung lässt sich nach dem Neustart erneut anzeigen.

Die vier Host-Ausgabetests, der gezielte Test zur aktuellen Plangeneration,
47 GTK-Tests und 33 Drohnensimulator-Tests bestanden. Die Prüfprotokolle
liegen unter `var/verification-task-output/`.
