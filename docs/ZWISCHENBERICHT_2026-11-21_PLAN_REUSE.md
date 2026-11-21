# Zwischenbericht vom 21. November 2026

## Fertige Teilergebnisse nach einer Neuplanung

Ein Ersatzplan kann Ergebnisse aus erfolgreich abgeschlossenen Schritten
seines direkten Vorgängers als Eingaben übernehmen. Der Betreiber aktiviert
das mit `plan.replanning.reuse_completed_exports` und einem Reasoner
für `arcs.plan_candidate.v2`. Der Planer benennt den
Vorgängerschritt und wählt von der Vorlage freigegebene Felder, die er im
eingefrorenen Neuplanungskontext erhalten hat. Die [Anleitung zur Neuplanung](../apps/adapter-host/examples/plan-replanning.md#fertige-teilergebnisse-übernehmen)
enthält ein Beispiel.

Das Ersatzplanartefakt `arcs.plan.v3` speichert die Quellschritte, Ergebnisversionen und
ausgewählten Felder. Bereits vorhandene Ergebnisse stehen sofort bereit.
Abhängigkeiten von neuen Schritten warten weiter auf deren Abschluss.
Die Runtime übernimmt die Bindungen und Aufgaben in derselben
Journaltransaktion. Jeder neue Schritt unterliegt seinen normalen
Reasoning-Budgets. Die Übernahme selbst startet keinen Reasoner für den
fertigen Vorgängerschritt.

## Geprüfte Abläufe

- In der ersten Generation liefert ein Schritt einen Wert. Ein abhängiger
  Schritt schlägt fehl. Der Ersatzplan verwendet den alten Wert in zwei
  neuen Schritten. Einer davon kombiniert ihn mit einem neuen Ergebnis.
- Der Test unterbricht die Übernahme ins Journal nach Veröffentlichung des
  Ersatzplanartefakts. Nach dem Neustart verwendet ARCS denselben Plan und
  dieselbe Ergebnisversion. Der alte Schritt und die Ersatzplanung werden
  dabei nicht erneut ausgeführt.
- ARCS weist Bezüge aus dem Erstplan, zu gescheiterten Schritten, auf
  private Felder und auf vom Kontextbudget ausgelassene Ergebnisse ab.
  Der Test prüft auch deaktivierte Übernahme und ein zu kleines
  Kontextbudget für gemischte Abhängigkeiten.

Ein Prozesslauf hat eine im vorherigen Hoststand begonnene Neuplanung
mit derselben Request-ID und Kontextversion fortgesetzt. In einem zweiten
Lauf hat der native Summenadapter den gespeicherten Kandidaten des
Vorgängerschritts als Eingabe verarbeitet.

Der Workspace-Lauf bestand 680 Rust-Tests. Der zusätzliche Test zur
Übernahme aus einem `arcs.plan.v3`-Vorgänger bestand im gezielten Lauf.
Auch die 48 GTK-Tests bestanden.

Der Zugriff umfasst den direkten Vorgänger derselben Aufgabe.
