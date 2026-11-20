# Zwischenbericht vom 21. November 2026

## Berichtsentwürfe aus Arbeitsplänen

Betreiber können in einer Schrittvorlage festlegen, welches Ergebnis ARCS
für welchen Output-Adapter aufbereiten soll. Beim Abschluss eines Plans
speichert die Runtime den Abschluss und die Berichtsvormerkungen gemeinsam.
Nach einer Neuplanung stammen die Entwürfe aus der erfolgreichen
Generation. Das gemeinsame [Architekturdokument](ARCHITECTURE.md) und der
[Implementierungsplan](IMPLEMENTATION_PLAN.md) beschreiben den Rahmen.

Der Core veröffentlicht den Bericht mit Verweisen auf die genaue
Ergebnis- und Planversion. Er übernimmt die freigegebenen Exportfelder der
Vorlage und das Vertrauensniveau des Ergebnisses. Die Runtime prüft den
konfigurierten Zielvertrag und bereitet je Durchlauf bis zu vier Entwürfe
vor. Das Journal hält bis zu 128 noch vorzubereitende Entwürfe. Bei voller
Warteschlange wartet der Planabschluss auf einen freien Platz.

Im Kontrollpanel öffnet der Betreiber den abgeschlossenen Plan und wählt den
gewünschten Entwurf. Die Vorschau zeigt den Inhalt für den festgelegten
Adapter. Erst die gespeicherte Freigabe mit Begründung gibt die konkrete
Ausgabe an die Zustellwarteschlange weiter. Eine erfolgreiche Zustellung
erscheint mit Quittung. Die [Anleitung zur Ausgabewarteschlange](../apps/adapter-host/examples/output-delivery-queue.md)
enthält die Konfiguration und die entsprechenden Monitor- und CLI-Befehle.

## Geprüfte Abläufe

- Ein Test lässt zunächst einen Plan trotz eines erfolgreichen Teilschritts
  scheitern und erstellt eine erfolgreiche Ersatzgeneration. Nur für deren
  Schritt entsteht ein Entwurf. Der Test unterbricht das Speichern der
  Vormerkung und später das Speichern des Vorschauzustands. Nach dem
  Neustart verwendet ARCS dieselbe Auftragsversion.
- Der Host weist eine Freigabe mit verändertem Ziel zurück. Mit der passenden
  Freigabe schreibt der echte Dateiadapter genau eine Datei. Deren Inhalt
  entspricht der Vorschau.
- Der GTK-Test wechselt zwischen zwei Entwürfen mit unterschiedlichen
  Zieladaptern. Der Dialog verwirft beim Wechsel die bisherige Vorschau.
  Nach der Freigabe erhält der ausgewählte Adapter ausschließlich die
  Exportfelder seines Schritts.

Die automatische Vorbereitung ist an konfigurierte Schrittvorlagen
gebunden. Die Zustellung erfordert eine ausdrückliche Freigabe pro Entwurf.
