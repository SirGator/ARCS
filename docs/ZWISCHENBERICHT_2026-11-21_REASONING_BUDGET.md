# Zwischenbericht vom 21. November 2026

## Reasoning-Kontingent für dauerhafte Aufgaben

Mit `max_reasoning_requests` kann eine Eingangsroute begrenzen, wie viele
unterschiedliche Reasoning-Anfragen ARCS je Aufgabe zulässt. Bei einem
Arbeitsplan teilen sich der Planer, parallele und abhängige Teilschritte
sowie alle Ersatzgenerationen ein Kontingent. Eigenständige Aufgaben und
einzelne Termine einer Prüfserie haben jeweils ihr eigenes Kontingent.

Die Runtime prüft die Grenze beim Binden der Anfrage im bestehenden
SQLite-Journal. Ein neuer Fallback belegt einen Platz. Ein Neustart lädt
eine bereits gebundene Anfrage unter derselben ID wieder. Scheitert das
Speichern der Bindung, beginnt der Adapteraufruf erst nach einer
erfolgreichen Wiederholung der Speicherung.

Vor einem freigegebenen Aktorauftrag sichert die Runtime den Platz für die
anschließende Wirkungsbewertung. Bei fehlendem Platz endet die Aufgabe
mit dem Status `budget_exhausted`. Das Kontrollpanel zeigt den Verbrauch
und das Limit in den Aufgabendetails. CLI und Monitor liefern dieselben
Werte in der Aufgabenwarteschlange. Die [Konfiguration und Aufrufsemantik](../apps/adapter-host/examples/plan-replanning.md#kontingent-für-reasoning-anfragen)
sind im Beispiel zur Neuplanung beschrieben.

## Geprüfte Abläufe

- Nach einem unterbrochenen Journal-Commit wartet der Planeraufruf bis
  zur erfolgreichen Speicherung. Bei einer verlorenen Antwort nutzt die
  Ersatzplanung nach dem Neustart ihren vorhandenen Platz. Das
  erschöpfte Kontingent verhindert einen zusätzlichen Teilschritt.
- Eine unabhängige Aufgabe liefert während dieser Planung ihr Ergebnis.
  Ein Fallback ohne freien Platz erreicht seinen Adapter nicht.
- Ein Teilschritt wartet einen simulierten Monat über einen Neustart hinweg.
  Der Planer und der bereits fertige Schritt bleiben angerechnet. Wenn der
  wartende Schritt und die Zusammenführung fertig sind, sind vier Plätze
  belegt.
- Ein simulierter Lüfter bleibt trotz erteilter Freigabe ohne
  Wirkungsbewertungskapazität unverändert. Mit freiem Platz für die
  Bewertung wird eine Aktion ausgeführt und ihre Wirkung belegt.

Auch mit dem Kontingent kann ein erneuter Versand derselben Request-ID nach
einem Verbindungsverlust beim Anbieter weitere Kosten erzeugen. Die
Abrechnung an der Anbietergrenze und Geldbudgets bleiben Teil der offenen
Anforderung LNG-06.
