# Zwischenbericht vom 21. November 2026

ARCS kann ein Softwareprüfungs-Ziel über mehrere Sitzungen verfolgen.
Der mitgelieferte Ablauf plant einen lokalen Unit-Test, wartet auf eine
menschliche Freigabe und wertet den tatsächlichen Ausführungsbeleg aus.
Er nutzt den vorhandenen Goal-Ablauf, den Act-Manager und einen externen
Regel-Reasoner.
[Anleitung zum Softwarebeispiel](../apps/adapter-host/examples/workspace-check.md#dauerhaftes-prüfziel-starten)

Eine Route kann den Erfolg an `execution_success_schema_id` binden.
Das Assessment muss die konkrete aktuelle Quittung zitieren. Bei einem
Erfolgsurteil prüft der Host ihren Payload, bevor er das Outcome speichert.
Das Beispiel verlangt den vorgesehenen Prüfnamen, den Status `passed`
und Exitcode 0. Konfigurierte Beobachtungen und Abfragen sind auch mit
einem Erfolgsschema Pflichtbelege.

## Geprüfte Abläufe

- Bei einem Fehler im lokalen Beispielcode hält ARCS den fehlgeschlagenen
  Test als Outcome fest. Nach einer im Test vorgenommenen Codekorrektur,
  einem Neustart und einem simulierten Monat fordert dieselbe Aufgabe
  eine neue Freigabe an. Der zweite echte Prozesslauf besteht und sein
  Artefakt trägt den Erfolg.
- Ein Assessment, das die Originaleingabe statt der aktuellen Quittung
  zitiert, wird abgewiesen. Auch ein Erfolgsurteil mit einem Beleg, der
  das Betreiberschema verfehlt, erzeugt keinen erfolgreichen Abschluss.
- Das mitgelieferte Beispiel wurde über die reguläre CLI und das
  GTK-Kontrollpanel bis zum gespeicherten Abschluss ausgeführt.

Drei neue Integrationstests, 69 bestehende Regressionstests der
Goal-Runtime und 33 Drohnensimulator-Tests bestanden. Der gezielte Test
für fehlende Nachbeobachtungen bestand auch am Endstand.

Der Schritt prüft die Wiederaufnahme und den Testnachweis aus TEST-02.
Im Monatstest stammt die Codekorrektur vom Testaufbau.
