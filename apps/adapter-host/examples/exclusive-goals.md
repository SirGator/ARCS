# Dauerhafte exklusive Goal-Reservierungen

Diese Erweiterung ergänzt `ARCHITECTURE.md`, v3 §7.2 PAR-04 und §7.3:
Kooperierende Goal-Abläufe können gemeinsame Ressourcen exklusiv reservieren.
Besitz und Freigaben liegen als versionierte `arcs.resource_reservation.v1`-
Artefakte im vorhandenen Core-Store. Auch verschiedene Hosts mit demselben
Core-Datenbankpfad sehen denselben Besitzer.

Die Ablaufreservierung koordiniert Goals mit derselben Ressourcendeklaration
und hält den Besitz bis zur Freigabe. Eine konfigurierte
[Act-Reservierungsprüfung](#reservierungsrevision-am-aktor) weist ältere
Befehle am Simulator ab, sobald er eine neuere Revision angenommen hat.
Für reale Geräte stehen die bestätigte Übergabe und die Absicherung
weiterer Befehlswege aus.

## Beispiel mit simuliertem Gerät

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/exclusive-goals/host.json \
  --runtime apps/adapter-host/config/exclusive-goals/runtime.json \
  --ticks 20
```

Zwei Goal-Routen verwenden denselben simulierten Ventilator und deklarieren
`device.fan-01`. `fan-owner` reserviert ihn vor der ersten Datenerfassung und
wartet auf konkrete menschliche Aktionsfreigabe. `fan-check` wartet auf die
Reservierung und erfasst noch keinen veralteten Vorzustand. Journale liegen
getrennt unter `var/exclusive-goals/`.

Die Freigabe erfolgt wie im [Goal-Beispiel](goal-plans.md) über
`--goal-command` mit `command: review`, aktueller Goal-Sequence und exakter
Kandidatenversion. Danach dieselbe Runtime erneut mit `--ticks 20` starten.
Der erste Ablauf schaltet den simulierten Ventilator ein, erfasst den
Nachzustand und lässt die Wirkung bewerten. Anschließend erhält `fan-check`
die Reservierung und erkennt anhand frischer Daten das bereits erfüllte Ziel.
Das Beispiel endet mit zwei abgeschlossenen Goals und einer Geräteaktion.
Es verwendet deterministische Rules-Reasoner und einen Simulator.

## Deklaration und Lebensdauer

In einer normalen Goal-Route oder freigegebenen dynamischen Goal-Vorlage:

```json
"goal": {
  "exclusive_resources": ["device.fan-01", "controller.shared"],
  "…": "bestehende Goal-Konfiguration"
}
```

Der Ausschnitt zeigt nur die neue Option. Die vollständige Konfiguration steht
in [`runtime.json`](../config/exclusive-goals/runtime.json).
Pro Goal sind 1–16 eindeutige Namen mit 1–64 ASCII-Buchstaben, Ziffern, `.`, `_`
oder `-` erlaubt; insgesamt höchstens 256 unterschiedliche Namen pro Runtime.
Die Option ist standardmäßig leer und bleibt dann in gespeicherten Definitionen
unsichtbar. Bestehende Konfigurationen behalten ihre Identität.

Alle kooperierenden Abläufe für dasselbe Gerät müssen dieselben Namen verwenden.
Die Namen kommen vom Betreiber; ein Planer kann sie nicht im Planvorschlag
überschreiben. Die eingefrorene Goal-Vorlage trägt sie im Planartefakt mit.
Die Verification muss die Aktionsziele passend zur Ressourcendeklaration
begrenzen; der allgemeine Core leitet keine Gerätenamen aus Aktionspayloads ab.

- Mehrere Ressourcen werden gemeinsam oder gar nicht übernommen. Wartende
  Goals halten keine Teilreservierungen und verbrauchen dafür keinen
  Reasoning-Worker. Sie erscheinen in `TickReport.waiting_resources`.
- Besitz beginnt vor der Datenerfassung. Ein wartender Nachfolger baut erst
  nach Übernahme seinen frischen Beobachtungszustand auf.
- Direkt aufeinanderfolgende Runden desselben Goals behalten die Reservierung,
  einschließlich der Zeit bis zur menschlichen Freigabe.
- Bei einer konfigurierten Rundenpause wird die Reservierung nach Speicherung
  des Warte-Checkpoints freigegeben. Nach dem Wecken beginnt eine neue Sitzung
  mit frischen Belegen und neuer Kandidatenfreigabe.
- Ein Neustart überträgt den Besitz nicht an einen anderen Auftrag. Dieselbe
  Sitzung verwendet ihre vorhandenen Revisionen weiter.

Jede Sitzung ist an Runtime-Definitionsversion, Eingabeversion, Auftragsnummer
und Sitzungsnummer gebunden. Sie ist kein frei verwendbares Geräte-Token.
Der Core prüft bei Änderungen die gelesenen aktuellen Revisionen in derselben
Schreibtransaktion. Mehrere Reservierungen und ihre Provenienzbeziehungen
werden atomar gespeichert. Externe Adapter dürfen die interne Publisher-ID
`arcs.runtime.reservations` nicht übernehmen.

## Freigabe und unklarer Aktionsausgang

Ein Abbruch vor einem möglichen Action-Dispatch kann die Reservierung
automatisch schließen. Nach einem Dispatch bleibt sie bei fehlender oder
unklarer Wirkungsbewertung erhalten. Ein Goal kann somit bereits terminal
sein, während die Ressource weiterhin reserviert ist. Ein Neustart entfernt
diese Sperre nicht.

Normal abgeschlossene Sitzungen mit passendem Outcome-Beleg werden automatisch
freigegeben und mit diesem Beleg verknüpft. Auch eine Ablehnung vor Dispatch
schließt die Reservierung direkt. Die operative Sitzung im Runtime-Journal wird erst nach dem
Core-Commit als geschlossen markiert. Scheitert dieser letzte Checkpoint,
holt die nächste Bereinigung den Abschluss nach. Die Core-Artefakte bleiben
die maßgebliche Besitzquelle.

Für terminale Goals mit ungeklärtem Ausgang gibt es einen vertrauenswürdigen
Anwendungseinstieg:

```text
reconcile_goal_resources(host, sequence, reservation_versions,
                         evidence_versions, actor, reason)
```

Der Aufrufer authentifiziert einen Menschen oder eine berechtigte Systeminstanz.
Die API verlangt einen terminalen Goal-Zustand, die exakten gehaltenen
Reservierungsrevisionen, vorhandene Core-Belege und eine Begründung.
Sie kann aus Belegreferenzen allein keinen sicheren physischen Stillstand
ableiten. Die Klärung und sichere Übergabe müssen vor der Bestätigung erfolgt sein.

Im laufenden GTK-Panel steht der Auftrag unter **Steuerung → Reservierung
für Ziel …**. Der Dialog **Ressourcen klären** zeigt die gehaltenen
Revisionen und vorhandene Ausführungs- und Wirkungsbelege der Sitzung.
Wähle kürzlich gespeicherte Belege oder füge ihre Versions-IDs ein.
Nach **Belege anzeigen** und Prüfung des Gerätezustands bestätigst du
die Übergabe mit einer Begründung. Eine Änderung der Belegauswahl verlangt
eine erneute Vorschau. Der Monitor stellt auch Reservierungen älterer
Aufträge bereit, die aus der Liste der letzten 50 Goals gefallen sind.

Für eine gestoppte Runtime steht derselbe Port über `--goal-command` bereit:

```json
{
  "command": "release_resources",
  "sequence": 1,
  "reservation_versions": ["<aktuelle gehaltene Reservierungs-Version>"],
  "evidence_versions": ["<geprüfte Core-Belegversion>"],
  "operator_id": "operator.simon",
  "reason": "Gerätezustand geklärt und Übergabe bestätigt"
}
```

Alle aktuell gehaltenen Revisionen der Sitzung müssen gemeinsam angegeben
werden. Modellakteure und Freigaben aktiver Goals werden abgewiesen. Die
Wiederholung derselben Bestätigung verwendet denselben Freigabebeleg, solange
dieser noch aktuell ist. Eine alte Bestätigung kann eine spätere Besitznahme
nicht aufheben. Eine freigegebene Sitzung kann keine Ressourcen erneut erwerben;
ein weiterer regulärer Goal-Abschnitt braucht eine neue Sitzung.

## Reservierungsrevision am Aktor

Im Lüftersimulator aktiviert `"fenced_resource": "device.fan-01"` in
`fan-profile.json` die Prüfung am Empfänger. Für die dazugehörige Goal-Route
ergänzt du `"device.fan-01"` in `goal.exclusive_resources` und setzt
`fenced_resource` für jede Regel, die `fan.set` aufruft, auf denselben
Ressourcenbezeichner. Eine getrennte Beispielinstallation verwendet
eigene Core-, Runtime- und Simulator-Journale.
Der Host prüft Manifest, Route und Besitz, bevor er die genaue gehaltene
Revision an den Aktor übergibt.

Der Simulator akzeptiert Wiederholungen derselben Invocation bei
gleicher gültiger Revision. Sobald er eine neuere Revision angenommen hat,
weist er ältere Aufträge auch nach einem Neustart ab. Der Vergleich findet in
derselben Transaktion wie die simulierte Gerätewirkung statt. Der
Ausführungsbeleg im Core verweist auf die Reservierung, unter der der
Auftrag angenommen wurde.

Der Simulator erfährt eine neue Revision beim nächsten angenommenen
Act-Auftrag. Eine Freigabe im Core installiert am Controller noch keine
Sperre gegen einen bereits unterwegs befindlichen Auftrag. Diese Lücke
betrifft die Übergabe vor dem ersten Act-Auftrag des Nachfolgers und
bleibt getrennte Architekturarbeit.
[Protokoll](../../../ADAPTER_PROTOCOL.md#reservierungsrevision-an-der-act-grenze)

## Status und Grenzen

`TaskRuntime::exclusive_resources(host)` liest die aktuellen Core-Belege der
konfigurierten Ressourcen. Monitor-Snapshots und CLI-Berichte mit vorhandenen
Reservierungen enthalten `resource_reservations` mit Besitzer, Zustand,
Version, Entscheidungsakteur, Begründung und Belegreferenzen.
Monitor-Snapshots enthalten zusätzlich `resource_release_reviews` mit den
lokalen terminalen Goals, ihren gehaltenen Revisionen und vorhandenen
Ausführungs- und Wirkungsreferenzen. Der Host ermittelt sie aus den gehaltenen
Versionen und der gebundenen Control-Sitzung.

Unter `ready.control.resource_release_review` kündigt der Monitor die
Vorschau an. Der lesende Goal-Befehl `inspect_resource_evidence` nimmt
`sequence`, `reservation_versions` und `evidence_versions` entgegen.
Die Antwort enthält Sequenz, angefragte Reservierungsrevisionen und die
unveränderten gespeicherten Artefakte in der Reihenfolge der Belegauswahl.
Das Belegobjekt ist auf 65.536 Byte begrenzt. Die CLI gibt es unter
`resource_evidence_review` aus. Die Übergabe verwendet anschließend
`release_resources` und dessen Vergleich der gehaltenen Versionen.

Das Runtime-Journal führt nur Sitzungszeiger und die begrenzte Bereinigung.
Der vorhandene Planabbruch aktualisiert weiterhin Goal-Checkpoints, damit eine
verspätete Freigabe keinen abgebrochenen Auftrag wieder aktiviert.

Offen bleiben zeitlich begrenzte Leases mit sicherer Übernahme,
die Bestätigung der Übergabe durch reale Controller, die Absicherung
beliebiger Act-Clients, verteilte
Koordination über getrennte Core-Datenbanken und ein allgemeines Vorrangsystem.
Auch diese Reservierung ist keine neue Action-Berechtigung; Grants,
Verification und Approval gelten weiterhin.

## Prüfung

```bash
cargo test --locked --offline -p arcs-core runtime::reservations
cargo test --locked --offline -p arcs-host --test exclusive_resources
```

Vier Core-Tests prüfen atomare Aufnahme, Versionsvergleiche, Transaktionsfehler,
konkurrierende Verbindungen, Wiederöffnung und abgewiesene Modellfreigaben.
Neun Prozess-Tests ergänzen konkurrierende Goals und Hosts, manuelle Freigaben,
Abbruch vor und nach Dispatch, bestätigte Klärung samt CLI-Wiederholung,
fortlaufende Runden, Monatswartephasen und dynamische Goal-Vorlagen.
