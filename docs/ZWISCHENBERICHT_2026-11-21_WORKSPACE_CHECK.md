# Zwischenbericht vom 21. November 2026

ARCS kann für Softwareprojekte benannte lokale Prüfungen über den
Act-Manager ausführen. Der Betreiber hinterlegt das Prüfprogramm und
seine Argumente im Adapterprofil. Der freigegebene Auftrag wählt eine
dieser Prüfungen. Der Core bewahrt den korrelierten Ausführungsbeleg
einschließlich Status und verfügbarem Exitcode auf.
[Anleitung zum Workspace-Check](../apps/adapter-host/examples/workspace-check.md)

Der Adapter speichert die Ausführungs-ID vor dem Start. Bei einem
unterbrochenen Core-Commit übernimmt ARCS die gespeicherte Adapterquittung.
Wenn nur die Startabsicht gesichert wurde, meldet der Adapter beim
Wiederanlauf `interrupted`. Beide Fälle verhindern einen zweiten Lauf
unter derselben ID.

Die Integrationstests starten einen echten Prozess im Projektverzeichnis.
Sie prüfen die Aktionsfreigabe, positive und negative Exitcodes, das
Prüfzeitlimit sowie getrennte Ausfälle beim Speichern in Core und Adapter.
Die Prüfung liefert einen Baustein für das mehrsitzige Softwareprojekt
aus TEST-02 der Architektur.
