# Zwischenbericht vom 22. November 2026

ARCS prüft vor der ersten Planerbindung die konkreten Binäreingaben
und alle beteiligten Adapter. Dazu gehören die aktuelle Lesefreigabe,
Objektzahl, Bytevolumen und die gemeinsame Anforderung an Ressourcenpools.
Der Core leitet die Deskriptoren aus denselben gespeicherten Referenzen ab
wie beim Dispatch.
Der spätere Dispatch prüft die aktuelle Kapazität erneut und übernimmt
anschließend die Ressourcen für den Worker.

Eine aktuell belegte Option wird wieder berücksichtigt, sobald ihre
Ressourcen frei werden. Ein dauerhaft ungeeigneter Planer wird für die vorliegenden Eingaben
als `incompatible` gekennzeichnet. Sind alle passenden Optionen ungeeignet,
endet die Aufgabe mit Fehler, bevor ein Planer dauerhaft gebunden wird.

## Nachweise

- Der Test schloss zwei Planer als getrennte Prozesse an. Ein
  Empfängerlimit unter der tatsächlichen Bildgröße führte zur Auswahl
  des zweiten Planers. Dasselbe galt bei einer Poolgrenze, die nur der
  zweite Planer gemeinsam mit der Kamera einhalten konnte.
- Beide Suchläufe fanden den Marker. Nach dem Neustart enthielt der Store
  denselben Auswahlbeleg und dieselben zwei Bewegungsquittungen.
- Passte das Bild in keinen der beiden Receiver-Grants, blieb die
  Drohnenposition erhalten. Der Core-Test weist zudem nach, dass eine
  inkompatible Option die Wartephase einer belegten passenden Option
  nicht beendet.

In ARCS bestanden 264 Core-Tests und 57 Host-Integrationstests.
Vier Such-Prozesstests bestanden im separaten Simulatorprojekt. Dessen
Quellstand liegt im lokalen Commit `6611723`.

[Konfiguration und Bedeutung der Auswahlzustände](../apps/adapter-host/examples/reasoner-routing.md#auswahlpolitik)
