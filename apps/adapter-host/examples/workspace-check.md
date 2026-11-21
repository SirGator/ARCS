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

```bash
cargo test -p arcs-workspace-check --locked --offline
```
