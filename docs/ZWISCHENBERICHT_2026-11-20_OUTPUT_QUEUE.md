# ARCS – Dauerhafte Ausgabewarteschlange, 20. November 2026

Der laufende Host nimmt bereits geprüfte und freigegebene Ausgabeaufträge
über seinen Monitor an. Die CLI unterstützt die Aufnahme bei gestopptem
Host. Das Runtime-Journal hält die Identität und den Zustellstatus fest.
Der Core prüft Verification, Approval und aktuelle Adapterberechtigungen
vor dem Versand und speichert die Quittung im Artifact-Netz.
[Ausgabewarteschlange](../apps/adapter-host/examples/output-delivery-queue.md)

Ein Test hält die Antwort des echten Dateiadapters nach dem Schreiben
zurück. Der Monitor beantwortet währenddessen Befehle und ein neu
eingereichter Reasoning-Auftrag erreicht die Prüfung. Ein zweiter Host
mit derselben Core-Datenbank wartet auf die Sitzung und übernimmt später
dieselbe Quittung. Nach dem Neustart
prüft der Test getrennt eine unterbrochene Quittungsspeicherung im Core
und einen unterbrochenen Ergebnis-Commit im Runtime-Journal. Der Adapter
erhält bei der Wiederaufnahme dieselbe Invocation-ID. Am Ende liegen
genau eine Datei und eine bestätigte Zustellung vor.

Die sieben Core-Output-Tests, drei Host-Output-Tests und 13 Pipeline-Tests
bestanden. Das GTK-Kontrollpanel bestand 46 Tests und der Drohnensimulator
33. Die Prüfprotokolle liegen unter `var/verification-output-queue/`.

Die Aufnahme verwendet vorhandene Approval-Artefakte. Automatische
Ausgaben aus Arbeitsplänen und die Bedienung der Ausgabewarteschlange im
GTK-Kontrollpanel stehen noch aus.
