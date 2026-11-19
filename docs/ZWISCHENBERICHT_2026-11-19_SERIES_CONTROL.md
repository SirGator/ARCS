# ARCS – Prüfserien steuern, 19. November 2026

Wiederkehrende Prüfungen können als ganze Serie pausiert, fortgesetzt und
endgültig abgebrochen werden. Die Steuerung ergänzt die dauerhaften
Wartezustände aus LNG-04 und LNG-05 sowie den Abbruch und die Wiederaufnahme
aus PAR-07 und PAR-10. [Architekturspezifikation](ARCHITECTURE.md)

Die CLI `--series-command` und die Host-API verwenden dieselbe Seriensequenz,
die erwartete Revision, eine eindeutige Anweisungs-ID und eine
Betreiberbegründung. Ein identischer erneuter Aufruf liefert seinen früheren
Beleg. Der aktuelle Serienzustand erscheint im Host-Bericht. Der Verlauf liest
Steuerungsbelege seitenweise. [Anwendung und Beispiele](../apps/adapter-host/examples/scheduled-runtime.md#ganze-prüfserien-steuern)

Die Serienpause hält die noch offene Prüfung im vorhandenen Dispatcher an.
Der Host kann bereits laufende Reasoning-Antworten während der Pause
speichern. Ein Neustart oder das Fortsetzen der Serie erzeugt für eine
gespeicherte Antwort keinen neuen Aufruf. Einzeln gesetzte Aufgabenpausen
bleiben dabei wirksam. Pausierte oder abgebrochene Serien stehen außerhalb des
Fälligkeitsindex. Die Fortsetzung rechnet mit dem ursprünglichen Zeitraster.

Ein Abbruch speichert Serienzustand, Steuerungsbeleg und gegebenenfalls den
Abbruch des offenen Auftrags in einer Transaktion. Danach verwirft der Host
eine verspätete Antwort und gibt die reservierten Worker-Ressourcen frei,
sobald der Aufruf endet. Beim Wechsel vom parallelen zum synchronen Betrieb
bleiben überholte und abgebrochene Fehlversuche als Historie erhalten. Sie
belegen keinen Queueplatz und werden nicht erneut eingeplant.

Die Journalmigration ergänzt Revision und Zustand direkt an der Serie, erhält
die Zeitraster und bisherigen Auftrags-IDs und ersetzt den Fälligkeitsindex
durch einen Index für aktive Serien. Der Core behält die vorhandenen
Reasoning-Ergebnisse. Die Änderung verwendet dieselbe Runtime wie die
Einzelaufgaben und Planaufträge.

Vier neue Prozesstests prüfen CLI und Journalmigration, Pause bei laufender
Antwort, das Zusammenspiel mit der Pause einer Einzelaufgabe, Rollback beim
Abbruch und den Moduswechsel nach fehlgeschlagenen Prüfungen. Der separate
CLI-Nachweis öffnet ein vom vorherigen Host-Binary erzeugtes Journal. Er
bestätigt die Steuerungsrevisionen 1, 2 und 3, die Fortsetzung am zweiten
Rasterintervall mit einem übersprungenen Termin und genau zwei
Reasoning-Aufrufe auch nach erneutem Öffnen. SQLite verwendet für die nächste
Fälligkeit den Index für aktive Serien. Lokale Protokolle liegen unter
`var/verification-series-control/`.

103 verschiedene Tests in neun Host-Testsuiten sind erfolgreich. Dazu zählen
Scheduler, Aufgabenpausen, Fristen, Abhängigkeiten, Prioritäten, parallele
Ausführung, dynamische Pläne und Neuplanung. Der Workspace-Build für
Binärprogramme und Beispiele sowie Format- und Diff-Prüfung sind erfolgreich.
Clippy meldet die beiden vorhandenen Warnungen im Core-Reasoning-Service.

Die Betriebsabnahme über Monate und individuelle Kalenderregeln stehen aus.
Die CLI übernimmt Anweisungen bei gestoppter Runtime. Für Änderungen im
laufenden Betrieb steht die API in der Anwendung zur Verfügung, die den Host
besitzt.
