# Dauerhafte Reasoner-Auswahl pro Aufgabe

Dieser Slice setzt Teile von `ARCHITECTURE.md`, v3 §9.2/§9.3 um: Eine Route
kann zwischen ausdrücklich zugelassenen Reasonern wählen. Der Core speichert
die Entscheidung vor dem ersten Aufruf als `arcs.reasoner_selection.v1` im
gemeinsamen Artifact-Netz. Ergebnisprüfung, Aktionsrechte und manuelle
Goal-Freigaben gelten weiterhin.

## Startbares Beispiel

```bash
cargo build --workspace --bins --locked --offline
./target/debug/arcs-host \
  --config apps/adapter-host/config/reasoner-routing/host.json \
  --runtime apps/adapter-host/config/reasoner-routing/runtime.json \
  --ticks 12
```

Ein unabhängiger Solver-Auftrag belegt zunächst den einzigen logischen
`analysis_slots`-Platz. Die zweite Route kann denselben Pool über ihren
bevorzugten Solver verwenden oder die freigegebene Modell-Fixture wählen.
Sie entscheidet sich für die verfügbare Fixture. Zwei Ergebnisse erreichen
`ready_for_review`; `reasoner_selections` zeigt genau eine gespeicherte Auswahl.
Die Daten liegen getrennt unter `var/reasoner-routing/`.

Der erneute Start mit denselben Dateien übernimmt dieselben Ergebnisse und
denselben Auswahlbeleg. Ein Monitorstart mit `--monitor --stop-file
/tmp/arcs-reasoner-routing.stop --ticks 12` zeigt die Auswahl ebenfalls; die
Stop-Datei darf vorher nicht existieren.

Die Modell-Fixture führt keine Modellinferenz aus und liefert den ersten
Listenwert. Für die Beispielwerte ist das nicht die optimale Lösung.
`ready_for_review` bestätigt deshalb nur einen validierten Kandidaten zur
Prüfung. Das Beispiel zeigt belegte Ressourcen, tatsächlichen alternativen
Dispatch und Wiederaufnahme; es misst keine Modellqualität.

## Auswahlpolitik

Die bisherigen Felder `reasoner`, `method` und `budget` definieren die erste
Option. Optional ergänzt `reasoner_selection` eine bis sieben weitere:

```json
{
  "reasoner_selection": {
    "alternatives": [{
      "capability": {
        "adapter_id": "example.model-fixture",
        "capability_id": "solution.propose"
      },
      "method": "learned",
      "budget": {
        "max_context_items": 1,
        "max_context_bytes": 8192,
        "max_output_bytes": 4096,
        "max_candidates": 1,
        "max_runtime_ms": 1000,
        "max_iterations": null,
        "max_output_tokens": null
      }
    }],
    "search_text": "nearest"
  }
}
```

Die vollständigen Routen stehen in [runtime.json](../config/reasoner-routing/runtime.json).
Alle Optionen werden schon beim Öffnen gegen Rolle, Ausgabeschema, Methode,
Budget und aktuelle Grants geprüft. Doppelte Capability-/Methodenpaare werden
abgewiesen. Adapterbeschreibungen bleiben ungeprüfte Behauptungen.

Die lexikalische Suche bleibt auf diese Optionen begrenzt. Alle Suchwörter
müssen passen. Unter aktuell verfügbaren Treffern gewinnt die höchste Relevanz;
bei Gleichstand gilt die Betreiberreihenfolge. Leerer Suchtext erlaubt alle
konfigurierten Optionen. Verbindung, Quarantäne, laufende Reasoning-Aufrufe und
die eigenen Poolanforderungen des Adapters bestimmen die beobachtete Verfügbarkeit.

Diese Beobachtung reserviert keine Ressourcen. Der tatsächliche Aufruf prüft
erneut Grants, Eingaben, Workerplätze und die gemeinsamen Anforderungen von
Reasoner und binären Datenquellen. Eine gewählte Option kann deshalb anschließend
warten oder an einem später erkennbaren Eingabe-/Kapazitätsfehler scheitern.

Sind alle passenden Optionen belegt oder unerreichbar, wartet die Aufgabe mit
`resource: "reasoner-selection"`, ohne Auswahlbeleg oder Worker. Gibt es keinen
inhaltlichen Treffer, schlägt diese Aufgabe ausdrücklich fehl. Unabhängige
Aufträge können weiterlaufen.

## Private Suchfelder

Optional ergänzt `search_input_fields` bis zu 16 ausdrücklich benannte,
oberste Eingabefelder. Jedes Feld muss einen vorhandenen String mit mindestens
einem Buchstaben oder einer Ziffer enthalten. Fehlende, leere oder ungültige
Felder führen zu einem Aufgabenfehler. Die gesamte Suchanfrage bleibt auf
4096 Bytes und die Grenzen der Capability-Suche beschränkt.

Diese Werte werden im Core für die Suche verwendet und im Auswahlbeleg
gespeichert. Sie gelangen dadurch nicht in den externen Reasoning-Kontext.
`payload_fields` und Memory-/Abhängigkeitsregeln steuern weiterhin unabhängig,
welche Daten exportiert werden. Wer ein Suchfeld zusätzlich unter
`payload_fields` freigibt, exportiert es ausdrücklich.

## Persistenz und Ausführung

Der Auswahlbeleg bindet Definition, Eingabeversion und Aufgabenidentität an
die vollständigen erlaubten Optionen, Beobachtungen und die gewählte Option.
Konkurrierende SQLite-Verbindungen einigen sich atomar auf den zuerst
gespeicherten Beleg. Ein verlorener Queue-Checkpoint kann ihn nicht ersetzen.
Aktuelle Grants und die unveränderte Auswahlpolitik werden beim Laden geprüft.

Nach dem Speichern bleibt die Auswahl über Neustarts und Goal-Runden fest.
Ein inzwischen freier anderer Reasoner übernimmt nicht stillschweigend. Auch
ein Ausfall des gewählten Adapters erzeugt keine neue Entscheidung. Eine
allgemeine versionierte Neuplanung bleibt weitere Arbeit.

Die Anfrage referenziert nur die Belegversion im reservierten Constraint
`arcs_reasoner_selection`. Der Core prüft Capability, Methode, Budget,
Zielschema und die ursprüngliche Eingabe gegen diesen Beleg. Er verknüpft ihn
über Provenance und `based_on`, ohne ein zusätzliches externes Kontextobjekt
oder eine Vertrauensaufwertung der Eingabedaten einzuführen. Bereits validierte
Ergebnisse können auch bei anderweitig belegtem Adapter wiederverwendet werden.
Der Publisher `arcs.reasoning.selection` ist für den Core reserviert.

Goals wählen erst beim tatsächlichen Eintritt in `Planning`, nach Datenerfassung
und einem gegebenenfalls bereits erfolgreichen initialen Zielcheck. Wartende
Goals binden dadurch keinen unnötigen Planer. Der feste Assessor bleibt separat.
Alle wählbaren Goal-Planer benötigen ein Ein-Kandidaten-Budget und genügend
Kontextplätze. Bei deklarierten Binäreingaben brauchen sie außerdem einen
ausreichenden Receiver-Grant für die Objektzahl; Bytes und konkrete Referenzen
prüft weiterhin die eigentliche Anfrage. Manuelle Approval und Verification
bleiben vor jeder Gerätewirkung wirksam.

Erzeugte Pläne behalten die erlaubten Optionen ihrer Betreiber-Vorlagen.
Joins und Kindaufnahme prüfen den tatsächlich gebundenen Ergebnisproduzenten.
Gesamtbudgets rechnen pro Aufruf konservativ mit den höchsten zulässigen
Laufzeit- und Tokenbudgets aller Optionen, auch bei mehrfachen Goal-Runden.

CLI und Monitor zeigen unter `reasoner_selections` höchstens 50 aktuelle oder
zuletzt bearbeitete Auswahlen mit Aufgabe, Route, Belegversion, Capability,
Methode, Budget und Relevanz. Suchtext und private Werte werden dort nicht
ausgegeben. Bestehende Routen ohne Auswahlpolitik behalten ihre bisherige
Identität und ihr Verhalten.

## Prüfung und offene Arbeit

```bash
cargo test --locked --offline -p arcs-core reasoning::selection
cargo test --locked --offline -p arcs-host --test reasoner_selection
```

Die Tests prüfen konkurrierende Auswahl, Recovery zwischen Core- und
Queue-Commit, blockierte Optionen, private Daten, isolierte Fehler, Cache-Replay,
alternative Root-/Schritt-/Goal-Planer, falsche Belege und konservative Budgets.
Der Goal-Prozesstest verlangt eine manuelle Freigabe und prüft anschließend
die bestätigte Wirkung am simulierten Gerät.

Gemessene Zuverlässigkeit, Kostenoptimierung, semantische Eignungsprüfung,
automatische Fähigkeitszertifizierung, Mehrfachbeauftragung mit Ergebnisvergleich
und allgemeine Werkzeugauswahl sind noch nicht implementiert.
