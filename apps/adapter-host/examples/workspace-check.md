# Lokale Softwareprüfungen mit dem Act-Manager

`arcs-workspace-check` führt benannte Prüfungen in einem konfigurierten
Arbeitsverzeichnis aus. Der Betreiber legt das Programm, seine Argumente
und das Zeitlimit fest. Der freizugebende Kandidat wählt den Namen aus
dieser Konfiguration. Das Ausführungsartefakt enthält den Prüfstatus,
den verfügbaren Exitcode und den Bezug zum freigegebenen Auftrag.

## Adapter konfigurieren

Baue den Adapter mit `cargo build -p arcs-workspace-check --locked --offline`.
Die Linux-Laufzeit verwendet `/usr/bin/timeout` als Supervisor. Ein Profil
für ein Python-Projekt kann so aussehen.

```json
{
  "adapter_id": "project.checks",
  "workspace": "./my-project",
  "checks": {
    "unit": {
      "program": "/usr/bin/python3",
      "args": ["-m", "unittest", "discover", "-s", "tests"],
      "timeout_ms": 30000
    }
  }
}
```

Relative Pfade in diesem Profil beziehen sich auf das Verzeichnis der
Profildatei. Trage die Binärdatei und das Profil in die Host-Konfiguration
ein. `args` enthält das Adapterjournal, beispielsweise
`[{"path":"workspace-check.sqlite"}]`. Verwende die
[beiden Adapter-Schemas](../../../adapters/workspace-check/schemas) in
`host.schemas`. Gib dem Adapter einen Grant mit
`producer_class: "executor"`, `enabled_capabilities: ["check.run"]` und
`granted_permissions: ["workspace.check"]`. Das Host-Zeitlimit für diesen
Adapter braucht Spielraum über dem längsten Prüfzeitlimit, damit der
Supervisor sein Ergebnis zurückmelden kann.

## Prüfung freigeben und auswerten

Der Kandidat verwendet `arcs.workspace_check_candidate.v1`, den Payload
`{"check":"unit"}` und die erforderliche Fähigkeit
`project.checks/check.run`. Die Ausführung durchläuft Verification,
Approval, Action-Materialisierung und die normale Act-Ausführung. Eine
Anwendung kann eine bereits freigegebene Action mit
`Host::execute_approved` übergeben.

`arcs.execution_result.workspace_check.v1` enthält unter
`result.status` einen der folgenden Werte.

- `passed` bedeutet, dass das Prüfprogramm mit Code 0 endete.
- `failed` bezeichnet einen sonstigen Fehlercode des Prüfprogramms.
- `timed_out_or_killed` bezeichnet einen Abbruch durch ein Signal oder
  einen damit überlappenden Exitcode des Supervisors.
- `runner_error` bezeichnet einen Start- oder Supervisorfehler oder
  dessen reservierten Exitcode.
- `interrupted` bedeutet, dass der Journalzustand nur die Startabsicht
  belegt. Die Runtime kann daraus keinen Testerfolg ableiten.

Die Quittung beschreibt die Ausführung im Arbeitsverzeichnis zum Zeitpunkt
des Laufs. Für eine revisionsgenaue Prüfung konfiguriert der Betreiber
einen eigenen Checkout oder einen Testbefehl, der die gewünschte Revision
prüft. Das Tool läuft unter dem Benutzer des Adapters. Standardausgaben
werden verworfen. Eine konfigurierte Prüfung kann detaillierte Testberichte
als Dateien im Projekt ablegen.

Die Ausführungs-ID wird vor dem Prozessstart im Adapterjournal gebunden.
Bei einem Wiederholungsaufruf verwendet der Adapter die gespeicherte
Quittung oder meldet `interrupted`. Ein neuer Prüflauf braucht einen neuen
freigegebenen Auftrag.

## Dauerhaftes Prüfziel starten

Das [mitgelieferte Beispiel](../config/workspace-goal/runtime.json)
plant einen echten Python-Unittest für einen kleinen Rechner. Sein
Regel-Reasoner bewertet die aktuelle Quittung. Die Route verlangt eine
menschliche Freigabe und ein Erfolgsschema, das den vorgesehenen Test,
`passed` und Exitcode 0 voraussetzt.

```bash
cargo build --workspace --bins --locked --offline
arcs_workspace_goal=apps/adapter-host/config/workspace-goal
./target/debug/arcs-host --config "$arcs_workspace_goal/host.json" \
  --runtime "$arcs_workspace_goal/runtime.json" --ticks 40
```

Ein frischer Lauf zeigt unter `goals` die Sequenz, den aktuellen
`candidate` und den Status `awaiting_approval`. Die [Goal-Review-CLI](goal-runtime.md)
speichert eine Freigabe für dieses Paar. Ein weiterer Start mit
`--ticks 40` führt die Prüfung aus. Alternativ lassen sich Host und
Runtime im Kontrollpanel laden und der Kandidat dort freigeben.

Das Journal erhält die Aufgabe über Neustarts. Nach einem fehlgeschlagenen
Test wartet die Beispielroute bis zum nächsten Versuch. Die nächste
Runde verlangt eine neue Freigabe. Die Anwendung oder der Betreiber
produziert die Codekorrektur zwischen den Prüfrunden.

```bash
cargo test -p arcs-workspace-check --locked --offline
cargo test -p arcs-host --test workspace_goal --locked --offline
```
