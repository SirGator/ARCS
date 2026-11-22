# Metaprompts für kleinere Modelle

Codex kann klar abgegrenzte Dokumentation, Routinecode und Unit-Tests
delegieren. Für jeden Auftrag wählt Codex das günstigste geeignete verfügbare
Modell und gibt ihm die nötigen Entscheidungen und Dateipfade. Codex prüft
den Diff und übernimmt Integration, Commit und Push.

## Standard

```text
Rolle: <Funktion des Entwicklungsagenten>.
Auftrag: <konkrete Änderung und Zielgruppe>.
Kontext: <Repository, relevante Dateien, bestätigte Fakten und Schnittstellen>.
Grenzen: <zulässige Änderungen, zu erhaltende Architektur und Verträge>.
Akzeptanzkriterien: <beobachtbare Bedingungen für die Abnahme>.
Verifikation: <gezielte Tests und git diff --check>.
Rückgabeformat: <Zwischenbericht und kurze Statusmeldung nach dem Format unten>.

Lies nur die nötigen Ausschnitte. Melde offene Architektur- oder
Berechtigungsentscheidungen an den Hauptagenten.
```

- **Dokumentation** – Beschreibe das bestätigte Verhalten am vorhandenen
  Einstiegspunkt und prüfe bei Bedarf Beispiele und Links.
- **Routinecode und Tests** – Nutze vorhandene Funktionen und Fixtures.
  Prüfe die konkrete Verhaltensänderung oder den zu verhindernden Rückfall.

## Nachweis im Zwischenbericht

- **Erhalten** – Modell, Auftrag, bereitgestellte Dateien und Vorgaben.
- **Erledigt** – tatsächliche Änderungen mit Dateipfaden.
- **Geprüft** – ausgeführte Prüfungen mit Ergebnis sowie offene Punkte.
- **Status** – `SUCCESS`, `PARTIAL` oder `FAILED`.

In den Nachweis gehören nur Projektinformationen, die für die Aufgabe
bereitgestellt wurden. Verweise auf Dateien und Abschnitte reichen aus.
