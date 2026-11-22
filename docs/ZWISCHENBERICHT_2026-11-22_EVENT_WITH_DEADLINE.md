# Zwischenbericht vom 22. November 2026

ARCS kann den Start einer Aufgabe oder einer neuen Zielrunde auf ein
autorisiertes Ereignis mit einem zusätzlichen Termin legen. Der Termin
wird bei der Aufnahme der Aufgabe oder beim Beginn der Wartephase
berechnet und mit dem Ereigniscursor gespeichert. Wartende Aufgaben
belegen keinen Reasoning-Worker.

Pro Wartephase entscheidet ein Aktivierungsbeleg. Eine passende Meldung
kann vor dem Termin freigeben. Ist der Termin bereits fällig, hat er
Vorrang vor Ereignissen, die der Scheduler noch nicht verarbeitet hat.
Die Berichte zählen den Wartezustand als `waiting_event` und zeigen
seinen Termin unter `fallback_at_unix_ms` sowie bei der nächsten
Fälligkeit. Für die Aktivierung und die Queue bleibt der Termin auch
nach einem Neustart nachvollziehbar.

## Nachweise

- Bei voller Queue weckte eine passende externe Meldung eine wartende
  Aufgabe vor Ablauf des gespeicherten Termins. Eine Meldung für ein
  anderes Projekt ließ die Aufgabe weiter warten.
- Trafen Signal und Termin im selben Tick aufeinander, gewann der
  Termin. Ein weiterer Host-Start löste keine Wiederholung aus.
- Eine Zielrunde ohne Signal wurde nach einem simulierten Monat und
  einem Neustart wieder aktiv. Sie erfasste frische Zustandsbelege
  und benötigte ihre eigene Freigabe. Erst danach führte der Simulator
  den zweiten Schritt aus und der Prüfnachweis bestätigte den Abschluss.
- Der laufende Host liefert den gespeicherten Termin über die
  Queue-Abfrage. Ein GTK-Testdisplay bestätigte, dass die Aufgabenansicht
  ihn mit „Ohne Ereignis ab“ anzeigt. Ungültige Schemas und zu lange
  Wartezeiten werden bei der Konfigurationsprüfung abgewiesen.

[Konfiguration und Beispiele](../apps/adapter-host/examples/scheduled-runtime.md#ereignis-mit-spätestem-termin)
