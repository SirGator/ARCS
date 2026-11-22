# ARCS-Zwischenbericht · 22. November 2026

## Transform-Verträge für Plan-Abhängigkeiten

Eine empfangende Vorlage kann pro registriertem Quellschema einen
Transform-Vertrag festlegen. Der Planer wählt weiterhin erlaubte Vorlagen und
Felder. Bei passendem Vertrag müssen die Kantenfelder gleichzeitig den
Quell-Export und die freigegebene Projektion erfüllen. Der Nachfolger erhält
das registrierte Ergebnis mit den ausgewählten Ausgabefeldern. Derselbe Weg
steht für freigegebene frühere Ergebnisse und Fehlerquittungen bereit.

Die Runtime verwendet die bestehende Wartestufe für Abhängigkeiten. Erst
nach Aktivierung und erfolgreichen Voraussetzungen startet sie die
Transform-Worker. Pause, Ablauf und Abbruch begrenzen deren Lebenszyklus.
Verwaiste Worker behalten ihre belegten Ressourcen bis zum Ende des Aufrufs.
Parallele Nachfolger können denselben laufenden Aufruf verwenden. Der
Monitor zeigt belegte Worker.

## Verifikation

269 Core-Tests und 105 Host-Prozesstests bestanden. Geprüft wurden die
Feldgrenzen vor und nach der Transformation, die Ablehnung einer fremden
Adapterrolle, der Wiederanlauf mit derselben Invocation-ID, die Verarbeitung
eines gepinnten Vorgängerergebnisses sowie Aktivierung und Abbruch. Der
Workspace bestand die Prüfung aller Targets.

## Delegierte Dokumentation

- **Erhalten** – Bonsai 2 27B erhielt die Felder und Freigaberegeln der
  Transform-Verträge sowie `docs/METAPROMPTS.md`.
- **Erledigt** – Kurzen Doku-Entwurf geliefert. Der Hauptagent prüfte den
  Entwurf und ergänzte ein Konfigurationsbeispiel.
- **Geprüft** – Feldnamen mit dem Auftrag abgeglichen.
- **Status** – `SUCCESS`.

## Delegierter Negativtest

- **Erhalten** – GPT‑5.6 Luna erhielt die Plan-Fixture und den Auftrag, eine
  vorhandene Input-Capability als ungültigen Transform-Vertrag zu prüfen.
- **Erledigt** – Einen Test in `apps/adapter-host/tests/dynamic_plans.rs`
  ergänzt. Er prüft die Ablehnung vor Anlage des Runtime-Journals.
- **Geprüft** – Der gezielte Offline-Test und `git diff --check` bestanden.
- **Status** – `SUCCESS`.
