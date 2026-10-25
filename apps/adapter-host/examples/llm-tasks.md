# Textaufgaben mit einem externen LLM-Dienst

`config/llm-tasks/host.json` verbindet zwei Instanzen des eigenständigen
Nachbarprojekts [`llm-api`](../../../../llm-api/README.md). Das Programm und
seine Anbieteranbindungen, Prompts und Profile liegen **außerhalb von ARCS**.
Die Modelldienste laufen lokal oder beim API-Anbieter. Weder Core noch Host
enthalten Modellaufrufe oder fachliches Sprachverständnis.

Unterstützte API-Formate: OpenAI-kompatible Chat Completions, OpenAI Responses,
Anthropic Messages, Gemini generateContent und Ollama Chat. Weitere Formate
lassen sich im externen Projekt ergänzen. Die allgemeinen ARCS-Verträge
bleiben unverändert.

## Start

Der normale Weg ist jetzt **Modelle** im Kontrollpanel: Verbindung anlegen,
**Verbindung testen**, **Speichern**, Modelle für Planung und Wirkungsprüfung
auswählen und **Für Aufgaben übernehmen** drücken. Schlüssel werden im
geöffneten Fenster gehalten oder optional im Systemschlüsselbund gespeichert.
Dieser Weg benötigt keine Umgebungsvariablen und erzeugt eine eigene
Startkonfiguration. Details stehen in der [Panel-Anleitung](../../control-panel/README.md#modellverbindungen-und-schlüssel).

Für eigene Profile und den direkten CLI-Start bleibt zusätzlich dieser Weg:

1. `ARCS_LLM_PROVIDER` und `ARCS_LLM_MODEL` im startenden Terminal setzen.
2. Für einen entfernten Dienst die passende Schlüsselvariable setzen;
   `ARCS_LLM_ENDPOINT` überschreibt optional den vollständigen Endpunkt.
3. Das Kontrollpanel aus demselben Terminal starten:

```bash
./arcs-control \
  --config apps/adapter-host/config/llm-tasks/host.json \
  --runtime apps/adapter-host/config/llm-tasks/runtime.json
```

Alternativ im neu gestarteten Panel **Aufgaben per Text · LLM-API** wählen.
Die vollständigen Einstellmöglichkeiten und Startbeispiele stehen in der
[externen Anleitung](../../../../llm-api/README.md).

Beim Start gibt es keine voreingestellten Aufgaben oder API-Aufrufe. Nach einer
Texteingabe führen `llm.assessor` und `llm.planner` über die gemeinsame Rolle
Reason den Zielcheck, den Handlungsvorschlag und die Wirkungsprüfung durch.
Beide Instanzen können unabhängig Anbieter, Modelle und Zugang wählen.
Die mitgelieferten Profile demonstrieren ausschließlich den simulierten Lüfter;
allgemeine Sprache erweitert nicht automatisch dessen Gerätefähigkeiten.

```text
Input → frische Quellen → externer LLM-Zielcheck → externer LLM-Vorschlag
      → Verification → konfigurierte Simulatorfreigabe → Act
      → frische Quellen → externe LLM-Wirkungsprüfung → weiter oder fertig
```

Grants verwenden `producer_class: "model"` und Methode `llm` mit höchstens
2048 Ausgabetokens und 60 Sekunden pro Invocation. Es gibt höchstens vier
Aktionsrunden. Bereits ein einzelner Schritt kann drei API-Aufrufe benötigen;
eine Aussage zur monetären Obergrenze ergibt sich daraus ohne Modellpreise
und Eingabetokenzählung nicht. Die bestehenden Freigaben erlauben nur die
simulierte boolesche Aktion für `fan-01`.

Journale und Core liegen unter `var/llm-tasks/`. Ein Anbieter-/Modellwechsel
benötigt eigene Reasoner-Journale; alte Antworten werden nicht mit einem
anderen Modell vermischt. Keys werden nur als Umgebung vererbt und nicht in
ARCS-Konfigurationen geschrieben. Ungültige Modellantworten werden abgewiesen;
Transportfehler nutzen den vorhandenen Runtime-Fehlerpfad. Der Adapter
verarbeitet aktuell Text/JSON, keine Kamera-Binärdaten.
