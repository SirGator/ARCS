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
des Adapters in der temporären Kopie. Für fremden oder generierten
Code steht die isolierte Prüffähigkeit zur Verfügung. Das Host-Zeitlimit
muss den Kopiervorgang und das Prüfzeitlimit abdecken.

`arcs.execution_result.workspace_revision_check.v1` hält den Commit
unter `result.revision` neben Prüfstatus und Exitcode fest. Bei
`interrupted` belegt der Commit die Zielrevision des Auftrags.
Ein gespeichertes Ergebnis bleibt bei einer Wiederaufnahme erhalten,
auch wenn sich die Referenz inzwischen verschoben hat.
## Diagnoseausgaben einer Revision erfassen

Ergänze das Adapterprofil um `"diagnostic_revision_checks": ["unit"]`,
um für die benannte Prüfung auch `check.revision.diagnostic` anzubieten.
Der Host lädt zusätzlich `revision-diagnostic-execution.schema.json`.
Der Grant gibt die neue Capability mit
`workspace.check_revision_diagnostics` frei. Der Kandidat enthält
weiterhin den Prüfnamen und den vollständigen Commit-Hash. Die
Prüfung braucht eine eigene Freigabe. Für die Erweiterung einer
bestehenden gebundenen Installation verwende eine neue Adapter-ID und
ein neues Journal.

`arcs.execution_result.workspace_revision_check.v2` behält
`result` für Prüfstatus, Revision und Exitcode. Bei
`failed` und `timed_out_or_killed` enthält `diagnostics`
zusätzlich `stdout` und `stderr`. Beide Proben enthalten
`text` und `truncated`. Der Adapter behält je Strom höchstens
die ersten 512 Eingabebytes. Er liest weitere Ausgabe während der
beaufsichtigten Ausführung ab, damit ein schreibintensiver Test
weiterlaufen kann. Der Adapter ersetzt ungültige UTF-8-Folgen
und ASCII-Steuerzeichen außer Zeilenumbrüchen, Wagenrücklauf und
Tabulatoren. Offen gebliebene Ströme gelten als unvollständig. Ein bestandener Test erzeugt nur die
Statusquittung. Bei Wiederaufnahme bleibt die gespeicherte
Diagnose zusammen mit der Quittung erhalten.

Für die Neuplanung kann der Betreiber
`replanning_failure_receipt_fields: ["result", "diagnostics"]`
an der Prüfvorlage setzen, wenn das Goal das v2-Ausführungsschema
exportiert. Mit `["result"]` erhält der Ersatzplaner weiterhin
nur die Prüfmetadaten. Die Diagnose enthält Text aus dem
konfigurierten Prüfprogramm. Wähle Programme, deren Ausgabe
für die freigegebenen Empfänger bestimmt ist.

[Konfiguration der Neuplanung](plan-replanning.md#geprüfte-fehlerquittungen-für-den-ersatzplaner)

## Eine Revision mit Bubblewrap isoliert prüfen

Das Adapterprofil aktiviert `check.revision.isolated` für ausgewählte
Prüfnamen. Die Fähigkeit braucht dieselbe Git-Arbeitsreferenz wie
eine revisionsgenaue Prüfung und einen vertrauenswürdigen
Bubblewrap-Launcher. Das getestete Linux-Profil benötigt
Benutzer- und Netzwerk-Namespaces.

```json
{
  "isolated_revision_checks": {
    "program": "/usr/bin/bwrap",
    "checks": ["unit"]
  }
}
```

Der Grant autorisiert `check.revision.isolated` mit
`workspace.check_revision_isolated`. Der Host verwendet
`arcs.workspace_revision_check_candidate.v1` und das Diagnose-
Ausführungsschema `arcs.execution_result.workspace_revision_check.v2`.
Die in `checks` genannten Programme behalten fest konfigurierte
Argumente. Ein isoliert konfigurierter Prüfname ist für die anderen
Prüffähigkeiten des Profils gesperrt.

Bubblewrap bindet die Git-Kopie schreibgeschützt unter `/workspace` ein.
Programme unter den gemounteten Systemverzeichnissen wie `/usr`
und `/bin` stehen ebenfalls schreibgeschützt bereit. `/tmp` ist ein
neues tmpfs mit 64 MiB. Das Profil verwendet eine leere
Prozessumgebung mit fest gesetztem Pfad und temporärem Home-Verzeichnis.
Der Prozess erhält eigene Netzwerk- und PID-Namespaces und startet
ohne zusätzliche Linux-Capabilities.

Der Adapter verlangt Bubblewraps separaten Exitbeleg. Ein fehlgeschlagener
Start liefert `runner_error` mit begrenzter Diagnose, soweit vorhanden.
Das Prüfprogramm wird in diesem Fall nicht außerhalb der Sandbox
gestartet. Das vorhandene Zeitlimit bleibt wirksam. Mit dem
optionalen Block `resources` lässt sich die isolierte Prüfung
zusätzlich pro Prozessbaum begrenzen.

Die beiden Prozess-Tests benötigen ein nutzbares Bubblewrap. Für einen
abweichenden Installationspfad kann der Tester `ARCS_TEST_BWRAP` setzen.

```bash
cargo test -p arcs-workspace-check --locked --offline bubblewrap_isolates -- --ignored
cargo test -p arcs-host --locked --offline --test workspace_goal \
  a_workspace_repair_can_use_only_isolated_checks -- --ignored
```

[Offizielle Bubblewrap-Dokumentation](https://github.com/containers/bubblewrap/blob/main/bwrap.xml)

### CPU-, Speicher- und Prozesslimits setzen

Ergänze in `isolated_revision_checks` den Block `resources`.
`parent` muss ein an den Adapterbetrieb delegiertes
cgroup-v2-Verzeichnis sein. Darin müssen die Controller `cpu`,
`memory` und `pids` für Kindgruppen aktiviert sein. Der Adapter
und die Prüfgruppen müssen im selben delegierten Bereich liegen.

```json
{
  "resources": {
    "parent": "/sys/fs/cgroup/path-to-delegated-workspace-checks",
    "cpu_percent": 50,
    "memory_bytes": 134217728,
    "max_tasks": 16
  }
}
```

`cpu_percent: 100` entspricht der Zeit eines logischen CPU-Kerns.
`memory_bytes` gilt für die Gruppe, der Swap-Verbrauch ist gesperrt.
`max_tasks` zählt Linux-Tasks einschließlich Threads. Die Grenzen
gelten für die Sandbox und alle Prüfnachfahren. Kann die Gruppe
nicht eingerichtet werden, startet die Prüfung nicht.
[Kernel-Dokumentation zu cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html)

Mit diesem Profil verwendet die Capability das Ergebnisschema
`arcs.execution_result.workspace_revision_check.v3`.
Lade `revision-resource-execution.schema.json` und aktualisiere
die betroffenen Goal-Erfolgsschemata, Exporte und Request-Feldbindungen. Verwende dafür eine neu gebundene
Adapterinstallation mit eigenem Journal.

`resources` in der Quittung enthält die eingestellten Grenzen und die
Kernelzähler für CPU-Drosselung, Speicherereignisse und abgewiesene
Task-Erzeugung. CPU-Drosselung kann bei einer bestandenen Prüfung
auftreten. Ein Speichermangel oder eine abgewiesene Task-Erzeugung
liefert `resource_limit`. Bei normalem Abschluss oder Timeout
beendet der Adapter verbleibende Gruppenprozesse und räumt die
Kindgruppe auf. Vor dem Prüfstart speichert das Adapterjournal
Name, Gerät und Inode der Kindgruppe. Nach einem abrupten Abbruch
bereinigt der Adapter beim Öffnen desselben Journals die eindeutig
zugeordneten Gruppen. Der erneute Aufruf einer abgebrochenen
Prüfung erhält `interrupted`, ohne den Prüfbefehl zu starten.

Gruppen ohne Journaleintrag bleiben unberührt. Bei einer
abweichenden Gruppenidentität meldet der Adapter einen Fehler
beim Start. Die Diagnose nennt die betroffene Gruppe.

Für den Kerneltest setze `ARCS_TEST_CHECK_CGROUP` auf einen leeren,
passend delegierten Prüfbereich und optional `ARCS_TEST_BWRAP`
auf den Pfad zum lokalen Bubblewrap. Dieselben Einstellungen
aktivieren die Limits im isolierten Ende-zu-Ende-Reparaturtest.

```bash
cargo test -p arcs-workspace-check --locked --offline \
  isolated_checks_enforce_and_report -- --ignored
cargo test -p arcs-workspace-check --locked --offline \
  a_killed_adapter_reaps_only_its_cgroup -- --ignored
cargo test -p arcs-host --locked --offline --test workspace_goal \
  a_workspace_repair_can_use_only_isolated_checks -- --ignored
```

## Quelltext einer Revision abrufen

Ein optionaler `source`-Block im Adapterprofil gibt einen lokalen
Git-Bereich für die Request-Capability `source.read` frei.

```json
{
  "reference": "refs/arcs/review",
  "files": ["calculator.py"]
}
```

Lade `source-request.schema.json` und `source.schema.json` in den Host
und autorisiere `source.read` mit `workspace.source_read`.
Setze im Grant `ingress_source_kind` auf `file`. Die Fähigkeit kann allein oder zusammen mit den Prüf- und Edit-Fähigkeiten
angeboten werden. Eine vorhandene gebundene Installation benötigt
für das zusätzliche Profil eine neue Adapter-ID und ein neues Journal.

Der Request nennt `revision` und `path`. Der vollständige Commit muss
beim ersten Abruf über die konfigurierte lokale Referenz erreichbar
sein. Der Pfad muss in `source.files` stehen und auf eine reguläre
UTF-8-Textdatei mit höchstens 32 KiB zeigen. Das Ergebnis enthält
`revision`, `path` und den exakten `text` aus dem Git-Blob. Für
die serialisierte Antwort gilt zusätzlich der Byte-Vertrag des Hosts.
Bei Wiederaufnahme bekommt der Core dieselbe gespeicherte
Antwort, auch wenn die Referenz inzwischen weitergezogen ist.

Ein Reparatur-Goal kann den Commit eines freigegebenen Vorgängers
in den Request übernehmen. Der Reasoner erhält so den Quelltext
des fehlgeschlagenen Stands als Version im Artifact-Netz.
[Konfiguration der Request-Bindung](goal-runtime.md#request-felder-aus-gebundenen-vorgängerartefakten)

## Korrektur und Prüfung in einem Plan verbinden

Der Rules-Reasoner bietet mit dem Profil `scenario: "workspace_repair"`
einen Beispielablauf für eine vorgegebene Textkorrektur. Die Eingabe
`arcs.task.workspace_repair.v1` enthält `base_commit`, `path`,
`replacement` und den Namen der Prüfung unter `check`. Der Reasoner
verwendet eine Planerroute mit `arcs.plan_candidate.v1` sowie zwei
Goal-Vorlagen namens `edit` und `check`. Beide brauchen manuelle Freigabe.

Die Edit-Vorlage verwendet `edit.apply`, ein Erfolgsschema für
`applied` oder `unchanged` sowie folgende Export-Einstellungen.

```json
{
  "export_fields": ["result"],
  "route": {
    "goal": {
      "export_execution_schema_id": "arcs.execution_result.workspace_edit.v1"
    }
  }
}
```

Die Check-Vorlage verwendet `check.revision` und ein Erfolgsschema für
die passende erfolgreiche Prüfquittung. Ihre Abhängigkeit vom
Edit-Schritt erlaubt dem Reasoner, den freigegebenen Commit aus
`result.commit` in den neuen Prüfkandidaten zu übernehmen. Die
Projektdatei und die Git-Referenz brauchen dafür keinen manuellen
Zwischenschritt. Bei einem Edit-Konflikt bleibt der Prüfschritt
blockiert. Der Integrationstest lädt die echten Adapter und zeigt
die vollständige Konfiguration.

```bash
cargo test -p arcs-host --locked --offline --test workspace_goal repair::
```

[Integrationstest und Beispielkonfiguration](../tests/workspace_goal/repair.rs)

## Einen lokalen Modellvorschlag für die Reparatur prüfen

Die Dateien `repair-model-profile.json`, `repair-model-grant.json`
und `repair-model-instructions.md` unter
`apps/adapter-host/config/workspace-goal/` beschreiben einen
Modelladapter für `arcs.workspace_edit_candidate.v1`.
Die reparierende Goal-Vorlage verwendet `method: "llm"`.
Sie erhält den Quelltext über `source.read` sowie die fehlgeschlagene
Testquittung über eine autorisierte `previous_failure`-Kante.
Das Modell schlägt die Textänderung vor. Die Action vergleicht über
`source_matches` den vorgeschlagenen Pfad und Basis-Commit mit
der gespeicherten Quelltextversion. Erst nach bestandener
Verification wartet der Host auf die Freigabe.

Der Prozess-Test verwendet die bereits installierte lokale
Ollama-Instanz und den externen Adapter aus dem Nachbarprojekt
`llm-api`. Setze bei Bedarf `ARCS_REPAIR_ENDPOINT` und
`ARCS_REPAIR_MODEL`. Der Test erstellt ein isoliertes Rechnerprojekt,
simuliert 30 Tage und begrenzt seine Testfreigaben auf die erwartete
Rechnerkorrektur. Die Prüfung startet über denselben Act-Pfad wie
ein normaler freigegebener Auftrag.

```bash
cargo test -p arcs-host --locked --offline --test workspace_goal \
  an_external_model_repairs -- --ignored --nocapture
```

[Konfiguration für gespeicherte Fehlerbelege](plan-replanning.md#fehlerbelege-an-einen-schritt-des-ersatzplans-weitergeben)

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
