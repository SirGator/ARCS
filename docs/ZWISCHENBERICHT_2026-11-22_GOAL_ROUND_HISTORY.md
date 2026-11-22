# Zwischenbericht vom 22. November 2026

ARCS speichert bewertete Zielrunden als `arcs.goal_round.v1` im
bestehenden Artifact-Netz. Ein Rundenbeleg verbindet die ursprüngliche
Aufgabe mit der gespeicherten Runtime-Definition, der Bewertung und bei
einer ausgeführten Aktion mit dem Wirkungsbeleg. Der Zielstand verweist
auf die neueste belegte Runde. Jede weitere Runde hält die genaue
Version ihres Vorgängers fest.

Der Gedächtnisabruf kann ausgewählte Felder früherer Runden laden.
Frische Zustandsdaten und die vorhandenen Freigaberegeln gelten auch
nach einer langen Wartephase. Bestehende Aufgaben können mit der nächsten
bewerteten Runde eine Historie beginnen.

## Nachweise

- Nach vier Runden über drei simulierte Monate und mehreren Neustarts
  führten die gespeicherten Verweise zum ursprünglichen Auftrag und zu den
  konkreten Bewertungen. Die Memory-Policy stellte frühere Runden mit
  den drei freigegebenen Feldern für Planung und Wirkungsbewertung bereit.
- Ein absichtlich fehlgeschlagener Commit der Wartephase ließ den
  Rundenbeleg im Core bestehen. Der Neustart übernahm dieselbe Version
  und führte die bereits bestätigte Aktion kein zweites Mal aus.
- Eine frische Prüfung konnte nach einer Pause feststellen, dass das
  Ziel bereits erreicht war. Ihr Beleg verwies auf die neue Bewertung.
  Eine Bewertung mit veralteten Daten wurde abgelehnt.

[Konfiguration und Datenvertrag](../apps/adapter-host/examples/goal-pauses.md#bewertete-runden-im-artifact-netz)
