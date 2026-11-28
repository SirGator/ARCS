# Zwischenbericht: Vertrauliche Fähigkeiten (28. November 2026)

Dieser Bericht beschreibt den siebten Slice der Zielbilderweiterung
([ZIELBILD_ERWEITERUNG_2026-11-27.md](ZIELBILD_ERWEITERUNG_2026-11-27.md)):
Geheimnisse außerhalb des Artefaktnetzes, eine Egress-Grenze für
Adapterinstallationen und die Durchsetzung beider Grenzen an der
Wirkungsgrenze.

## Ausgangslage

Die bisherigen Schnitte hatten die Schutzstufe eines Artefakts, die
Herabstufung durch den Store, die erzwungene Ableitung und den
Default-Deny am Modellrand umgesetzt (`PRIV-01`, `PRIV-02`, `PRIV-03`).
Offen blieben drei Lücken, die das Zielbild ausdrücklich fordert:

1. Ein Geheimnis war kein eigener Gegenstand. Jeder Wert, der in ein
   Artefakt geschrieben wurde, war Teil des Artefaktnetzes und damit
   grundsätzlich für Memory, Retrieval und ein Modell erreichbar.
2. Es gab keine ausdrückliche Grenze für die externe Umgebung einer
   Installation. Der Grant regelte Fähigkeiten und Datenklassen, aber
   nicht, wohin Daten einer Installation gelangen dürfen.
3. Das Zielbild verlangt, dass „Fehlerausgaben, Telemetrie und
   Suchanfragen ebenfalls Datenabflüsse" sind. Für ausgehende Aufträge
   gab es dafür keine durchgängige Prüfung.

## Änderungen

### Geheimnisse außerhalb des Artefaktnetzes (`PRIV-04`)

* Neues Kernmodul `crates/arcs-core/src/privacy/`:
  * `SecretVault` hält die Werte in einer eigenen SQLite-Datei. Die
    Tabelle ist kein Artefakt; Retrieval, Memory und Graphbeziehungen
    erreichen sie nicht.
  * `SecretBroker` vermittelt Werte über `SecretGrant`. Eine Freigabe ist
    ein versioniertes Artefakt (`arcs.secret_grant.v1`) und nennt Zweck,
    Ressource, Ablaufzeit und Höchstzahl der Nutzungen. Sie trägt nie den
    Wert.
  * Die Kennung eines Geheimnisses wird vergeben und nicht aus dem Wert
    abgeleitet. Ein ableitbarer Wert wäre selbst eine Offenlegung.
* Nutzung: Ein Auftrag kann an der Stelle eines Werts die Markierung
  `{"$secret": "<grant_id>"}` tragen. Der gespeicherte Auftrag behält die
  Referenz. Erst der ausgehende Auftrag trägt den Wert, und nur für die
  Nutzung, für die die Freigabe ausgestellt wurde (Zweck, Ressource,
  Ablauf, Nutzungszahl).
* Das Artefaktnetz bleibt frei vom Wert: Der Fingerabdruck einer
  Invocation wird über den Auftrag *ohne* eingesetzte Geheimnisse
  gebildet. Ein gespeicherter Digest eines Geheimnisses wäre selbst eine
  Offenlegung; der bisherige 64-Bit-Digest wäre zudem erratbar.
* Antworten externer Adapter können Werte zurückspiegeln. Vor dem
  Speichern werden sie redigiert.

### Egress-Grenze einer Installation (`PRIV-05`)

* `BoundaryPolicy` im `AdapterGrant`: Netzwerk, erlaubte Ziele und
  erlaubte Dateisystemwurzeln. Ohne Grenze gilt Default-Deny.
* Ein Auftrag kann ein Ziel mit `{"$destination": "<host>"}` benennen.
  Die Grenze wird geprüft, bevor der Endpunkt den Auftrag sieht. Ein
  Auftrag kann seine Reichweite also nicht selbst erweitern.
* Eine strukturell unzulässige Grenze macht die Installation unbrauchbar:
  Die Registry verweigert die Registrierung, statt mit einer ungeklärten
  Grenze zu laufen.

### Bedienoberfläche

* Neue Konfiguration `secret_vault` im Host: eigene Datei neben der
  Datenbank, außerhalb des Artefaktnetzes.
* Neue Befehle `--secret-command`: `register`, `issue`, `inspect`,
  `revoke`. Der Bericht nennt Zweck, Ressource, Ablauf und Nutzungen, nie
  den Wert.
* Ohne Tresor kennt die Instanz kein Geheimnis: Eine Geheimnisreferenz
  in einem Auftrag wird dann nicht gesendet (Fail-closed).

## Nachweis

| Anforderung | Nachweis | Test |
| --- | --- | --- |
| Geheimnis verlässt den Tresor nicht | Der Wert erscheint in keinem Artefakt des Netzes und in keinem Bericht | `privacy::tests::a_secret_stays_outside_the_artifact_network`, `secret_grants::an_operator_registers_a_secret_and_issues_a_scoped_grant` |
| Freigabe ist an Zweck und Ressource gebunden | Fremder Zweck und fremde Ressource werden abgelehnt | `privacy::tests::a_grant_only_serves_its_purpose_and_resource` |
| Freigabe ist kurzlebig und begrenzt | Ablauf, Nutzungszahl und Widerruf werden durchgesetzt | `privacy::tests::an_expired_grant_is_refused_and_marked`, `an_exhausted_and_a_revoked_grant_are_refused` |
| Agenten erhalten Fähigkeiten statt lesbarer Werte | Der Wert erscheint nur im ausgehenden Auftrag, nicht im gespeicherten | `action::execution::tests::a_secret_reaches_the_adapter_but_not_the_artifact_network` |
| Kein Tresor, keine Sendung | Ein Auftrag mit Geheimnisreferenz wird ohne Tresor nicht gesendet | `action::execution::tests::an_order_with_a_secret_is_refused_without_a_vault` |
| Antworten spiegeln keine Geheimnisse | Rückspiegelte Werte werden vor dem Speichern ersetzt | `privacy::tests::an_answer_that_echoes_a_secret_is_redacted` |
| Default-Deny der Egress-Grenze | Ohne Grenze ist jedes Ziel verboten | `privacy::tests::a_boundary_denies_every_destination_by_default` |
| Der Auftrag erweitert seine Reichweite nicht | Ein nicht freigegebenes Ziel stoppt den Auftrag vor dem Adapter | `action::execution::tests::an_order_that_names_a_denied_destination_is_never_sent` |
| Freigegebenes Ziel wird bedient | Der erlaubte Auftrag läuft | `action::execution::tests::an_order_that_names_a_permitted_destination_is_sent` |
| Unzulässige Grenze wird abgelehnt | Die Registry verweigert die Registrierung | `AdapterRegistryError::InvalidBoundary` |
| Bedienoberfläche ohne Rückgabewert | Bericht und Netz kennen nur Zweck, Ressource und Zustand | `secret_grants::an_operator_registers_a_secret_and_issues_a_scoped_grant` |

## Erkenntnisse

1. **Ein Geheimnis braucht keinen eigenen Artefakttyp, aber einen eigenen
   Speicherort.** Der Wert darf nicht durch das Artefaktnetw hindurch, weil
   jeder Pfad, der ein Artefakt lesen kann, sonst auch ein Geheimnis lesen
   kann. Der Tresor ist deshalb eine eigene Datei, nicht eine Tabelle im
   Core-Speicher.
2. **Die Freigabe ist das prüfbare Objekt, nicht der Wert.** Eine
   Freigabe lässt sich prüfen, widerrufen und in der Historie lesen; der
   Wert nicht. Das ist die Trennung, die `PRIV-04` verlangt.
3. **Der Fingerabdruck einer Invocation ist selbst ein Datenabfluss.**
   Der bisherige 64-Bit-Digest über den vollständigen Auftrag wäre für ein
   kurzes Geheimnis erratbar. Die Grenze bildet den Fingerabdruck nun
   über den Auftrag ohne eingesetzte Werte.
4. **Eine Markierung im Auftrag ist die kleinste gemeinsame Grenze.**
   Zwei Markierungen (`$secret`, `$destination`) tragen beide Fälle ohne
   Schemaänderung: Das Schema bleibt unverändert, die Prüfung liegt an
   der Wirkungsgrenze und nicht in der Fachlogik.
5. **Default-Deny braucht einen sichtbaren Fehler.** Ohne Tresor und ohne
   Zielverzeichnis wäre ein Auftrag mit Geheimnis stillschweigend ohne
   Wert gelaufen. Der Fail-closed-Fehler macht den fehlenden Tresor
   sichtbar.

## Offene Punkte

* `PRIV-05` ist nur für die Grenzen umgesetzt, die ARCS selbst kontrolliert:
  Netzwerk, Ziele, Dateisystemwurzeln, Umgebung und Auftragsinhalt. Eine
  Sandbox für den Kindprozess (Netzwerknamespace, Dateisystemzugriff des
  Kindes) bleibt eine Betriebsaufgabe des Betreibers.
* `PRIV-08` bis `PRIV-10` (Abstraktion, symbolisches Reasoning, lokale
  Rekonstruktion) sind offen. Der Core besitzt jetzt die Grenze, an der
  eine abstrahierte Aufgabe eingesetzt werden könnte.
* `PRIV-11` (Confidential Computing, Differential Privacy, MPC, HE) bleibt
  eine Forschungsaufgabe.
* Die Protokollgrenze zwischen Core und Adapter kann Geheimnisse nicht
  transportieren, wenn ein Adapter sie in einem eigenen Kanal erwartet.
  Heute trägt der ausgehende Auftrag den Wert; das ist bewusst so,
  weil die Übertragung an genau eine freigegebene Fähigkeit gebunden
  ist.

## Reproduktion

```bash
cargo test -p arcs-core --lib privacy
cargo test -p arcs-core --lib action::execution
cargo test -p arcs-host --test secret_grants
```
