# Simulierter Lüfter und Temperaturquelle

Der eigenständige Adapter bietet Observe, Request und Act in einem Prozess.
Profile, Geräteschemas und der persistente Simulator gehören zu diesem Paket;
der ARCS-Core enthält keine Lüfterlogik.

```bash
arcs-simulated-fan <profile.json> <journal.sqlite> [--adapter-id <id>]
```

Ohne explizite ID wird die bisherige Installation `demo.fan` verwendet.
Mit `--adapter-id fan.left` und `--adapter-id fan.right` lassen sich zwei
Instanzen desselben Programms anschließen. Die jeweiligen Betreiber-Grants
müssen die passende ID tragen. Lokale Capability-Namen wie `fan.set` dürfen
gleich sein; das Aufrufziel besteht aus Adapter-ID und Capability-ID.

Jede Installation benötigt ein eigenes Journal. Die Adapter-ID wird darin
dauerhaft gebunden. Eine andere ID wird auch nach Neustart abgewiesen.
Vorhandene Journale ohne ID-Bindung gehören zur bisherigen Standardinstallation
`demo.fan`; ihr Zustand wird bei der Übernahme erhalten.

Der [Mehrfachanschluss-Nachweis](../../apps/adapter-host/examples/multiple-adapters.md)
verbindet zwei Lüfter mit gleichen lokalen Gerätenamen, getrennten Zuständen
und eigenen Freigaben. Die
[Bewegungs- und Wirkungspipeline des Lüfterbeispiels](../../ADAPTER_PROTOCOL.md)
beschreibt Verification, Approval, Action und Outcome. Der allgemeine Host
erteilt keine automatische Freigabe.

Mit `"fenced_resource": "device.fan-01"` im Profil verlangt `fan.set` die
Reservierungsrevision dieser Ressource. Der allgemeine Host muss dieselbe
Ressource am Goal und seiner Aktionsregel deklarieren. Der Simulator
speichert die höchste angenommene Revision, Geräteänderung, Ergebnis und
Observation in einer Transaktion. Nach der Aktivierung bindet das Journal
das Fencing an dieselbe konfigurierte Ressource. Die vollständige
Konfiguration steht im [Goal-Beispiel](../../apps/adapter-host/examples/exclusive-goals.md#reservierungsrevision-am-aktor).

Der Simulator speichert `close_fence` dauerhaft, bevor er bestätigt.
Anschließend weist er dieselbe Revision auch bei Wiederöffnung und vor
dem ersten Auftrag des Nachfolgers ab. Der Lüfterzustand bleibt bis zur
nächsten genehmigten Aktion bestehen.
