# SCHEMA_CONTRACT

## Zweck

Diese Datei definiert die harte Regel fuer Schema- und Commit-Gates in ARCS.

Sie beantwortet drei Fragen:

1. Was darf nach Ingress noch ins System?
2. Welche Mindestbedingungen gelten vor einem Commit?
3. Was ist heute schon im Code erzwungen und was ist noch offen?

---

## Grundregel

Kein Artefakt darf committed werden, wenn es nicht gegen seine `schema_id`
und die zugehoerigen Governance-Regeln validiert wurde.

Kurzform:

```text
unknown schema_id = reject
invalid payload   = reject
unknown/fail gate = block
```

---

## V1-Vertrag

Ab Ingress gilt fuer ARCS V1:

- kein untypisierter JSON-Blob mehr im Core-Pfad
- jedes Artefakt hat eine bekannte `schema_id`
- jedes Artefakt hat einen gueltigen Basis-Aufbau
- jedes Payload muss gegen sein spezifisches Schema pruefbar sein
- Verifikation darf bei `fail` oder `unknown` nie in Execution uebergehen
- LLM-Output ist nur Vorschlag, nie Autoritaet

---

## Commit-Voraussetzungen

Ein Artefakt darf nur Teil eines Commit-Bundles sein, wenn mindestens diese
Bedingungen gelten:

1. `schema_id` ist gesetzt.
2. Die `schema_id` ist in der `SchemaRegistry` bekannt.
3. Das Artefakt ist strukturell als `ArtifactVersion` gueltig.
4. Das Payload ist gegen das referenzierte Schema gueltig.
5. `stream_key` ist gesetzt.
6. `created_by`, `source` und `trust` sind vorhanden und gueltig.
7. Abgeleitete Artefakte haben `provenance.parents`.
8. Referenzen auf andere Artefakte sind aufloesbar oder fuehren zu einem Block.
9. Das `CommitBundle` enthaelt Artefakte und Events atomar.
10. Das Bundle verletzt keine lokalen Konsistenzregeln oder Optimistic Locks.

---

## Verboten

- Commit mit unbekannter `schema_id`
- Commit eines Payloads, das sein Schema nicht erfuellt
- Event ohne zugehoeriges Artefakt
- Artefakt ohne zugehoeriges Event innerhalb derselben Commit-Grenze
- Ableitung von `action`, `approval`, `policy` oder `execution_result` direkt aus LLM-Output
- Parsing-Heuristiken im Materializer
- Demo-Bypass, der invalides Artefaktmaterial trotzdem in den Store bringt

---

## LLM- und Interpretationsregeln

Fuer die Interpretationskette gilt zusaetzlich:

- Rohtext wird ueber den externen `/interpret`-Pfad verarbeitet.
- Das Ergebnis muss schema-konform als Proposal zurueckkommen.
- Das Proposal darf keine direkte Exekution ausloesen.
- `trust.level=low` bleibt Pflicht fuer Modell-Output.
- Ungueltiger Modell- oder Parser-Output wird rejected oder blockiert.

Siehe auch `GOVERNANCE.md` fuer die LLM-Commit-Regeln.

---

## Commit-Boundary

Schema-Gates stehen nicht allein. Sie gelten zusammen mit der Commit-Boundary:

```text
artifact_versions + events gehoeren zusammen
=> ein CommitBundle
=> ein atomarer Commit
=> kein Halb-Commit
```

Das ist nicht nur ein Store-Detail, sondern Teil des Vertrags.

---

## Was der aktuelle Code bereits erzwingt

| Bereich | Aktueller Stand | Referenz |
|---|---|---|
| Schema-Registry kennt keine leere ID | erzwungen | `tests/test_schema_registry.cpp` |
| Doppelte Schema-Registrierung wird abgelehnt | erzwungen | `tests/test_schema_registry.cpp` |
| Ingress kann Schema-Validierung nutzen | vorhanden | `src/ingress/src/ingress_validator.cpp` |
| Verification hat einen `SchemaVerifier` | vorhanden | `src/verification/src/schema_verifier.cpp` |
| `unknown` / `fail` blocken im Governance-Modell | dokumentiert und in Verifier-Flow angelegt | `docs/GOVERNANCE.md`, `src/verification/` |
| Commit ohne Versions oder Events wird im Store abgelehnt | erzwungen | `src/store/src/store_memory.cpp`, `src/store/src/store_sqlite.cpp` |
| Doppelte IDs im Bundle werden abgelehnt | erzwungen | `src/store/src/store_memory.cpp`, `src/store/src/store_sqlite.cpp` |
| Optimistic Lock wird vor Commit geprueft | erzwungen | `src/store/src/optimistic_lock.cpp` |
| Interpretationspfad ist fail-closed testbar | erzwungen und getestet | `tools/interpretation_worker/tests/test_full_stack_e2e.py` |
| Ungueltiger LLM-Kandidat soll vor Execution rejected werden | als Testregel vorhanden | `tests/test_llm_isolation.cpp` |

---

## Was noch nicht hart genug ist

Diese Punkte sind fuer den gewuenschten Vertrag noch nicht stark genug im Code
oder nicht zentral genug dokumentiert:

- Kein zentraler, eindeutig dokumentierter Commit-Gate, der fuer jeden Artefakt-Commit die `SchemaRegistry` zwingend nutzt.
- Nicht jeder heutige Demo- oder Test-Flow beweist bereits, dass invalides Artefaktmaterial niemals committed werden kann.
- LLM-Provenance ist in `GOVERNANCE.md` als Pflicht beschrieben, aber nicht hart durch den Worker erzwungen.
- `STATE.md`, `MVP_FLOW.md` und `TEST_MATRIX.md` fehlen noch als zusammenhaengender Garantie-Ueberblick.

---

## V1-Done-Kriterien fuer diesen Vertrag

Der Schema-Contract gilt erst dann als sauber umgesetzt, wenn diese Punkte gruen
sind:

- unbekannte `schema_id` wird vor Commit rejected
- invalides Payload wird vor Commit rejected
- CommitBundle bleibt atomar
- kein Event ohne Artefakt, kein Artefakt ohne Event
- Verifier koennen invalides oder unvollstaendiges Material nicht in Execution durchlassen
- Interpretationspfad bleibt fail-closed
- Tests beweisen die Gates, nicht nur die Happy Paths

---

## Naechste direkte Anschlussarbeit

1. `MVP_FLOW.md` schreiben.
2. `TEST_MATRIX.md` einfuehren.
3. `docs/SPECIFICATION.md` auf diesen Vertrag referenzieren.
4. Commit-Pfad im Code auf einen klaren Schema-Gate pruefen und haerten.
