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

Bei `check.run` beschreibt die Quittung die Ausführung im
Arbeitsverzeichnis zum Zeitpunkt des Laufs. Das Tool läuft unter dem
Benutzer des Adapters. Standardausgaben werden verworfen. Eine
konfigurierte Prüfung kann detaillierte Testberichte als Dateien im
Projekt ablegen.

Die Ausführungs-ID wird vor dem Prozessstart im Adapterjournal gebunden.
Bei einem Wiederholungsaufruf verwendet der Adapter die gespeicherte
Quittung oder meldet `interrupted`. Ein neuer Prüflauf braucht einen neuen
freigegebenen Auftrag.

## Freigegebene Codekorrekturen als Git-Versionen speichern

Mit der optionalen `edit`-Konfiguration bietet derselbe Adapter die
Fähigkeit `edit.apply` an. `workspace` muss dann auf die Wurzel eines
lokalen Git-Repositories zeigen. Beispielsweise kann die lokale
Arbeitsreferenz einmal vom aktuellen HEAD aus angelegt werden.

```bash
git -C my-project update-ref refs/arcs/review HEAD ''
```

Ergänze das Profil um die Referenz und die erlaubten Dateien.

```json
{
  "reference": "refs/arcs/review",
  "files": ["calculator.py"]
}
```

Der Host lädt `edit-candidate.schema.json` und
`edit-execution.schema.json` zusätzlich. Der Executor-Grant
autorisiert `edit.apply` mit `workspace.edit`. Beide Fähigkeiten des
Adapters können in derselben Sitzung verwendet werden. Für eine reine
Edit-Installation kann `checks` leer sein. Profil und Journal
gehören zu derselben Installation.

Der Kandidat für `arcs.workspace_edit_candidate.v1` enthält den vollständigen
Basis-Commit unter `base_commit`, den freigegebenen Pfad unter `path` sowie
den vollständigen neuen UTF-8-Text unter `replacement`. Die Grenze liegt
bei 32 KiB pro Textdatei. Git übernimmt die übrigen Dateien aus dem
Basis-Commit. Nach Verification und Freigabe veröffentlicht der Adapter
den Commit auf `refs/arcs/review`. Diese Referenz dient als Arbeitsstand
für nachfolgende Werkzeuge.

Das `arcs.execution_result.workspace_edit.v1`-Artefakt enthält den
Pfad, die Basis und den Commit. Bei `applied` ist der Commit veröffentlicht,
bei `unchanged` war der Text bereits identisch. Bei `conflict` enthält
`commit` den beobachteten neueren Stand. Eine Korrektur dafür benötigt
einen neuen Kandidaten mit passender Basis und eigener Freigabe.

## Einen bestimmten Git-Commit prüfen

Ergänze das Profil mit `"revision_checks": ["unit"]`. Die Liste wählt
Prüfprogramme aus `checks` aus und benötigt eine konfigurierte
`edit.reference`. Lade `revision-candidate.schema.json` und
`revision-execution.schema.json` in den Host. Gib `check.revision` mit
`workspace.check_revision` im Grant frei. Eine Installation, die
ausschließlich revisionsgenau prüfen soll, kann den Grant für
`check.run` weglassen. Verwende für die Erweiterung einer bereits
gebundenen Installation eine neue Adapter-ID und ein neues Journal,
damit die bisherigen Quittungen über die alte Installation verfügbar
bleiben.

Der Kandidat verwendet `arcs.workspace_revision_check_candidate.v1`
mit `check` und `revision`. Übernimm als `revision` den vollständigen
Commit-Hash aus `result.commit` einer Edit-Quittung mit Status
`applied` oder `unchanged`. Die Prüfung braucht eine eigene Freigabe.
Der Commit muss beim Start von der konfigurierten ARCS-Referenz aus
erreichbar sein. Damit lassen sich auch frühere Commits prüfen,
während die Arbeitsreferenz weiterläuft.

Der Adapter kopiert die versionierten regulären Dateien direkt aus
Git-Blobs in ein temporäres Verzeichnis. Er erhält den Inhalt und das
Ausführbarkeitsbit der Dateien. Git-Filter und Exportattribute werden
dabei umgangen. Bäume mit Symlinks oder Submodulen werden abgewiesen.
Das Limit beträgt 50.000 Dateien, 64 MiB je Datei, 512 MiB insgesamt
und 8 MiB für die Baumliste. Das Prüfprogramm arbeitet als Benutzer
des Adapters in der temporären Kopie. Eine Betriebssystem-Sandbox
ist dafür weiterhin offen. Das Host-Zeitlimit muss den Kopiervorgang
und das Prüfzeitlimit abdecken.

`arcs.execution_result.workspace_revision_check.v1` hält den Commit
unter `result.revision` neben Prüfstatus und Exitcode fest. Bei
`interrupted` belegt der Commit die Zielrevision des Auftrags.
Ein gespeichertes Ergebnis bleibt bei einer Wiederaufnahme erhalten,
auch wenn sich die Referenz inzwischen verschoben hat.
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
