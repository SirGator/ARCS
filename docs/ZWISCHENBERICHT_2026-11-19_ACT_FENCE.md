# ARCS – Reservierungsrevision am Aktor, 19. November 2026

Eine Fähigkeit vom Typ `fenced_act` gibt eine benannte exklusive Ressource
im Manifest an. Die Goal-Aktionsregel führt denselben Namen. Der Core
prüft die genaue gehaltene Revision vor der Übergabe, bindet sie an den
Invocation-Fingerprint und verknüpft den Ausführungsbeleg mit dem
Reservierungsartefakt. Er überträgt die Revision im optionalen Aufruf
`execute_fenced`. [Adapterprotokoll](../ADAPTER_PROTOCOL.md#reservierungsrevision-an-der-act-grenze)

Der Lüftersimulator hält die ausstellende Core-Kennung und die höchste
angenommene Revision im Gerätejournal. Vergleich, Änderung des
Gerätezustands, Antwort und Observation liegen in einer Transaktion.
Ältere Aufträge, eine andere Core-Kennung und eine unpassende
Reservierungsversion werden auch nach erneutem Öffnen abgewiesen.
[Simulatorkonfiguration](../apps/adapter-host/examples/exclusive-goals.md#reservierungsrevision-am-aktor)

Die Tests prüfen die Versionsbindung am normalen Goal-Einstieg,
Wiederholung ohne zweite Gerätewirkung, Neustart, widersprüchliche
Revisionen und Rollback des simulierten Effekts. Die Arbeit betrifft
PAR-04, PAR-10 und §7.3.
[Architekturspezifikation](ARCHITECTURE.md)

369 Rust-Tests waren erfolgreich. Davon liefen 290 in Core, Protokoll,
SDK und Lüftersimulator, 72 im allgemeinen Host und sieben in der
Lüfter-Demo. Im separaten Drohnensimulator bestanden 33 Tests mit dem
bisherigen Act-Aufruf. Workspace-Build und Formatprüfung liefen
erfolgreich. Clippy meldete die beiden vorhandenen Warnungen im
Core-Reasoning-Service. Lokale Logs liegen unter `var/verification-act-fence/`.

Die Sperre gegen ältere Aufträge greift am Simulator nach Annahme der
neueren Revision. Damit die Übergabe bereits bei der Freigabe greift,
muss der Controller das Schließen einer Reservierung bestätigen.
Dieser Übergabeschritt, die Übertragung auf reale Controller und
die Langzeitabnahme stehen aus.
