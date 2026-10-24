# Einfache Raum- und Bewegungssimulation

`arcs-room-sim` ist eine reine Rust-Bibliothek mit Serde als einziger
Abhängigkeit. Sie kennt weder ARCS-Artefakte noch Adapter oder Freigaben.
`Room::moved` berechnet eine neue Pose oder weist die Bewegung ab.

- Rechteckiger Raum in Metern, Bewegung bei fester Flughöhe.
- Pose: `x_m`, `y_m`, `yaw_deg`; Blickrichtung normalisiert auf `[0, 360)`.
- Befehle: `dx_m`, `dy_m`, `dyaw_deg`. Verschiebungen beziehen sich auf
  Raumachsen, nicht auf die Blickrichtung.
- Begrenzte Schrittlänge und Drehung, keine wirkungslosen Nullbefehle.
- Rechteckige Hindernisse und ein konservativer achsenparalleler
  Sicherheitsabstand `clearance_m` zur Drohne.
- Die gesamte Bewegungsstrecke wird mit den erweiterten Hindernisflächen
  geprüft. Eine freie Endposition hinter einem Hindernis reicht nicht.
  Berührung einer Sicherheitsgrenze wird abgewiesen.

Eine ungültige Bewegung verändert keine Pose. Die Anwendung muss ein Szenario
zuerst mit `Room::validate` prüfen; `moved` prüft Ausgangs- und Zielpose.
Die Berechnung hat keine Zeitachse, Geschwindigkeit, Beschleunigung oder
realistische Flugphysik. Es gibt noch keine Kamera, Zielobjekte oder Anzeige.

Das Beispielprofil liegt beim
[Drohnenadapter](../../adapters/simulated-drone/config/profile.json).
Dieser übernimmt Persistenz und Protokollzugang, die
[Beispielanwendung](../../apps/drone-demo/README.md) die Freigabepolicy.

```bash
cargo test --locked -p arcs-room-sim
```
