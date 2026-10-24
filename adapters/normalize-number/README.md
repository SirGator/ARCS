# Allgemeiner Transform: Zahlen normalisieren

Dieser eigenständige Beispieladapter verarbeitet geprüfte JSON-Daten über
den allgemeinen Transform-Anschluss. Er kennt keine Geräte, Drohnen oder
ARCS-Core-Interna. Rechenregel und fachliche Schemas liegen in diesem Paket.

## Start

Im Workspace-Verzeichnis:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host -- \
  --config apps/adapter-host/config/transform.json
```

Die Konfiguration übergibt:

```json
{"value":310.0,"scale":0.1,"offset":0.0,"unit":"Cel"}
```

Der Adapter berechnet `value * scale + offset` und liefert:

```json
{"value":31.0,"unit":"Cel"}
```

Ein zweiter Anwendungsfall kann beispielsweise `value=10`, `scale=2`,
`offset=-1`, `unit="m/s"` verwenden und erhält `19 m/s`. Die Einheit wird
ausdrücklich angegeben. Eine automatische Erkennung oder Umrechnung von
Einheiten findet nicht statt. Nicht endliche Eingaben, Überläufe, leere
Einheiten und Einheiten mit Steuerzeichen werden abgewiesen.

## Allgemeiner Ablauf

Mehrere Instanzen können dieselbe lokale Capability `number.normalize`
anbieten. `arcs-normalize-number --adapter-id normalize.left` wählt einen
eigenen Installationsnamen; der zugehörige Betreiber-Grant muss denselben
Namen verwenden. Ohne Argument bleibt `example.normalize` erhalten.
Die Bibliothek bietet dafür `NormalizerInstance::new(AdapterId)`; der
bisherige `Normalizer` behält sein Standardverhalten. Ein vollständiges
[Mehrfachanschluss-Beispiel](../../apps/adapter-host/examples/multiple-adapters.md)
verwendet zwei Normalisierer gleichzeitig.

1. Der Host persistiert die vollständige Auftragsdefinition und ihren Input.
2. `TransformService` prüft die aktuelle Capability-Freigabe, beide
   Schema-Verträge und die Größe der gespeicherten Eingabe.
3. Der externe Prozess erhält ausschließlich den typisierten Transform-Auftrag
   und liefert einen korrelierten Payload zurück.
4. Der Core validiert das Ergebnis und setzt IDs, Herkunft, Trust und
   Provenienz. Resultat, `derived_from`-Relation und erfolgreicher
   Invocation-State werden in einer Transaktion gespeichert.

Die Beispielkonfiguration erzeugt sechs Artefakte: Definition, Input,
drei Invocation-Zustände und das Resultat. Die aktuelle CLI-Ausgabe enthält
das Ergebnis unter `adapters[].transforms`. Der World-State wird durch die
Ableitung nicht automatisch verändert. Es entstehen keine Actions oder
Freigaben.

## Wiederholung und Änderungen

`normalize-1` bezeichnet eine dauerhafte Auftragsdefinition. Ein Neustart
mit derselben Definition liefert dieselbe gespeicherte Ergebnisversion.
Geänderte Parameter, Schemas oder Zieladapter unter derselben ID werden
abgewiesen. Für einen neuen Auftrag ist eine neue ID erforderlich.

Alle fachlichen Rechenparameter stehen im Input. Der Adapter hat keinen
versteckten Geräte- oder Profilzustand und erzeugt keine externen Wirkungen.
Ein unterbrochener Aufruf kann mit derselben Invocation-ID wiederholt
werden. Der Host führt keine automatischen Wiederholungen oder Reconnects
durch. Eine veränderte Rechenimplementierung benötigt eine neue
Adapterversion; sie bildet eine neue Aufrufidentität.

Erfolgreiche Ergebnisse werden weiterhin gegen aktuelle Grants geprüft.
Ein inzwischen unzulässiges Trust-Niveau oder engere Ergebnislimits können
das Wiederverwenden des Ergebnisses verhindern.

## Bibliotheks-API und Grenzen

Anwendungen können mit `Host::transform(&CapabilityRef, &VersionId,
&SchemaId)` ein bereits gespeichertes Artefakt verarbeiten. Das funktioniert
auch mit Eingaben anderer Adapter. `transform_configured(id)` verwendet
dagegen einen Inline-Input aus der Konfiguration und ergänzt den CLI-Bericht.
Der Core enthält die Verarbeitungskontrolle; er hängt nicht von diesem
Beispielpaket ab.

Der erste Transform-Auftrag verarbeitet genau eine Artefaktversion. Deren
Payload darf strukturierte Daten oder Datenreferenzen enthalten. Ein
automatischer Abruf fremder Referenzen oder das Weiterreichen kompletter
Binärdateien an einen Transform ist noch nicht implementiert. Anwendungen
müssen solche Datenwege ausdrücklich orchestrieren. Transform-Quellen
können eigene unveränderliche Ausgabedaten über `read_data` bereitstellen,
wenn dafür separate Grants und Datenlimits vorliegen.

Die allgemeine Pipeline garantiert Schema-Prüfung, Herkunft und kontrollierte
Persistenz. Die fachliche Korrektheit eines anderen Transforms muss dessen
Implementierung beziehungsweise eine gesonderte Verifikation nachweisen.
