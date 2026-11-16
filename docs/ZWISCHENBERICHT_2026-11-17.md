# ARCS – Zwischenbericht vom 17. November 2026

**Implementierungsstand: `8ba870e` · Branch: `master`**

Seit dem [ausführlichen Zwischenbericht vom 16. November](ZWISCHENBERICHT_2026-11-16.md)
sind keine weiteren Codeänderungen hinzugekommen. Die dort beschriebenen neun
Entwicklungsschritte wurden mit Commit `8ba870e` zusammengefasst und auf
`origin/master` veröffentlicht. Diese Fortschreibung dokumentiert denselben
Funktionsstand und die nächsten Prioritäten.

## Bestätigter Funktionsstand

- Begrenztes paralleles Reasoning mit isolierten Adapter-Sessions, Deadlines
  und persistenten Aufträgen.
- Gedächtnisabruf aus dem gemeinsamen Artifact-Netz mit erlaubten Scopes und
  eingefrorener Kontextauswahl.
- Termin- und Ereignisaktivierung sowie wiederholte Wartephasen innerhalb
  von Zielaufgaben, einschließlich Wiederaufnahme nach Neustarts.
- Abhängigkeitsgraphen mit parallelen Zweigen und ausgewähltem Ergebnisexport.
- Dynamisch vorgeschlagene Pläne aus freigegebenen Vorlagen mit Prüfung,
  Gesamtbudgets, Planartefakten und atomarer Aufnahme der Kindaufträge.
- Dauerhafter Planabbruch und Goal-Schritte mit Verification, konkreter
  menschlicher Freigabe und abschließender Wirkungsbewertung.
- Eingeschränkte Prozessumgebungen und gezielte Modellschlüsselfreigaben.

Maßgeblich bleiben [ARCHITECTURE.md](ARCHITECTURE.md) und der
[Implementierungsplan](IMPLEMENTATION_PLAN.md). Das Ziel eines universellen
Assistenten mit realer Geräteanbindung und langfristigen Projekten ist
weiterhin in Entwicklung.

## Prüfbasis

Die letzten vollständigen Prüfungen des unveränderten Implementierungsstands
ergaben **419 bestandene Rust-Tests** sowie **29 bestandene Python-Tests** bei
**11 übersprungenen umgebungsabhängigen Desktoptests**. Build, Formatierung
und Clippy waren erfolgreich; Clippy meldete zwei bereits vorhandene Warnungen.

Das CLI-Beispiel mit einem simulierten Ventilator zeigte vor Freigabe keine
Aktion und nach Ausführung sowie Neustart genau eine Aktion. Die Planung wurde
dabei nur einmal ausgeführt. Weitere Prozess-Tests prüfen unter anderem
simulierte Monatswartephasen, neue Freigaben und unterbrochene Checkpoints.

Diese Ergebnisse stammen aus dem vorherigen vollständigen Prüflauf. Für diese
reine Dokumentationsfortschreibung werden Formatierung, Diff und Dokumentlinks
geprüft; die funktionalen Tests werden nicht als neu ausgeführt ausgewiesen.
Ein realer mehrmonatiger Dauerbetrieb, universelle LLM-Planung und reale
Drohnensteuerung sind damit weiterhin nicht nachgewiesen.

## Nächste Prioritäten

1. Den bestehenden Planvertrag mit echten externen Modellen an überprüfbaren
   Aufgaben erproben und Qualität, Laufzeit, Kosten und Eingriffe erfassen.
2. Allgemeine Ressourcenreservierungen für Geräte, GPU, RAM und APIs ergänzen,
   bevor gleichzeitig steuernde Geräteaufgaben ausgeweitet werden.
3. Ein versioniertes Zielmodell sowie kontrollierte Neuplanung auf den
   bestehenden Planartefakten und Ergebnisbelegen entwickeln.
4. Anspruchsvollere Simulator-Aufgaben und anschließende Dauerläufe mit realen
   Unterbrechungen durchführen.

Die konkreten nächsten Implementierungsschritte sind damit benannt; die
offenen Gesamtabnahmen und Grenzen bleiben im ausführlichen Bericht und im
Implementierungsplan dokumentiert.
