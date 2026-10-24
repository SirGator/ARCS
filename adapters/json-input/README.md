# Allgemeiner JSON-Input-Adapter

`arcs-json-input <profile.json> <journal.sqlite>` veröffentlicht zunächst die
konfigurierte Liste von JSON-Ereignissen über die Input-Rolle des gemeinsamen SDK.
Ein Profil nennt Adapter-ID, Capability-ID, ein Eingabeschema und Ereignisse
mit `id`, `subject`, `reference` und `payload`. Ein vollständiges Beispiel
liegt unter `apps/adapter-host/config/roles/input-profile.json`.

Der Adapter bewahrt jedes Ereignis in einer SQLite-Outbox auf und bietet es
bis zum Input-ACK erneut an. ACKs sind wiederholbar; unbekannte IDs werden
abgewiesen. Profilidentität und ACK-Stand bleiben nach Neustart erhalten.
Ein geändertes Profil an einem bestehenden Journal wird abgewiesen.

Der Core entscheidet anhand des Betreiber-Grants über Schema, Herkunft und
Trust. Er speichert Ereignis und Message-ID-Beleg vor dem ACK. Inputs ändern
den World-State nicht automatisch. Der Adapter kennt weder Core noch
fachliche Aufgabenlogik. Er ist eine reproduzierbare Eingangsquelle und
noch kein laufender Chat-, HTTP- oder Dateiwächterdienst.

Start, Wiederholung und Reasoning stehen in der
[vollständigen Anleitung](../../apps/adapter-host/examples/reason-task.md).

Neue Ereignisse können während des laufenden Adapterprozesses hinzugefügt werden:

```bash
./target/debug/arcs-json-input --enqueue \
  apps/adapter-host/config/roles/input-profile.json \
  var/roles-input.sqlite \
  apps/adapter-host/config/roles/input-live-event.json
```

Die Ereignisdatei enthält `id`, `subject`, `reference` und `payload`. Identische
Wiederholungen sind idempotent und setzen ein vorhandenes ACK nicht zurück.
Geänderter Inhalt unter derselben ID wird abgewiesen. Das gebundene Profil
bleibt unverändert; die Queue wird über eine eigene SQLite-Verbindung erweitert.
Ein Ablauf mit laufendem Host steht im
[Runtime-Beispiel](../../apps/adapter-host/examples/task-runtime.md).
