# GOVERNANCE

## Trust-Modell

| Level | source_class | Typische Quellen | Konsequenz |
|-------|-------------|-----------------|------------|
| **high** | human | Menschliche Freigabe, signierte Quellen | Kann Policy ändern, Approval erteilen |
| **medium** | system | Bekannte interne Systeme | Kann Artefakte committen, keine Policy-Änderung |
| **low** | model / external | LLM-Output, Web, unbekannte Externe | Nur Vorschläge, immer Verifier-Prüfung |

## LLM-Commit-Regeln

| LLM darf committen | LLM darf NICHT committen |
|-------------------|-------------------------|
| claim (als claim, nicht fact) | policy |
| option (als proposal, trust.level=low) | permission_grant |
| raw input + external API contract | task |
| match_result (wenn deterministic engine) | approval |
| assumption_set (als Vorschlag) | action |
| | execution_result |

## LLM-Provenance (Pflicht)

Für jeden LLM-involvierten Commit muss Model-Provenance gespeichert werden:

```json
{
  "provenance": {
    "models_used": [{
      "name": "model-identifier",
      "prompt_hash": "sha256:...",
      "inputs": ["a_task_...", "a_world_state_..."],
      "temperature": 0.2,
      "raw_output_hash": "sha256:..."
    }]
  }
}
```

> Du musst nicht reproduzieren können. Du musst beweisen können, warum du etwas übernommen hast.

## Approval-Regeln

### TOCTOU-Schutz

Approval ist an eine konkrete `policy_ref` (artifact_id + version_id) gebunden. Wenn Policy zwischen Approval und Ausführung wechselt: blocken und neu verifizieren.

### Clock-Ownership

- Clock-Ownership liegt beim Core (**ApprovalVerifier**)
- Executor macht nur einen finalen `expired => abort` Guard
- `expires_at` ist absolute UTC time
- Bei Replay: `validity at time of event` — nicht aktueller Stand
- TimeSource ist ein injiziertes Interface (nie `Date.now()` direkt)

## Policy & Permissions

### Trennung

| | policy | permission_grant |
|--|--------|------------------|
| Inhalt | Regeln, Constraints, Action-Allowlist, Verifier-Config | Principal → Capability Mapping |
| Änderungsfrequenz | Selten | Häufiger |
| Authority | policy:edit + second approver (V2) | perm:grant + approval |

### Policy-Struktur (V1)

```json
{
  "type": "policy",
  "payload": {
    "capabilities": ["exec:report_emit", "exec:file_write", "approve:high_risk"],
    "constraints": {
      "shell": { "allow_cmd": ["git", "python"] },
      "net": { "allow_domains": ["api.example.com"] },
      "file": { "allow_roots": ["/sandbox/work"] }
    },
    "verifier_rules": {
      "hard_checks": ["schema", "permission", "scope", "reference_integrity", "approval"],
      "soft_checks": ["risk_level", "confidence"]
    },
    "approval_required_for": ["safety_level:high", "exec:shell", "exec:file_write"]
  }
}
```

## Change Control für Policy-Änderungen

Änderungen an policy und permission_grant sind selbst Actions und brauchen denselben Governance-Pfad:

1. Änderungs-Option erstellen
2. **AuthorityVerifier** prüft: Actor hat `policy:edit` capability
3. VerificationReport: pass
4. Human Approval (mandatory)
5. Action: policy_update
6. Executor: PolicyUpdateExecutor
7. Neues policy Artefakt committed

## Drei harte Betriebsregeln

### Regel 1: Commit-Boundary (Transaktion)

Alles was zusammengehört wird in EINER Transaktion committed: artifact_versions + events. Kein Halb-Commit. Kein Event ohne Artefakt. Kein Artefakt ohne Event.

### Regel 2: Reducer-Regel für Head

Head wird durch Reducer bestimmt, nicht durch "latest version". Reducer sortieren nach Log-Reihenfolge (append index) oder causal links — NICHT nach occurred_at.

### Regel 3: At-least-once + Idempotenz

action_id ist der Idempotency-Key. Jeder Executor prüft persistent. Doppelte Ausführung: no-op oder safe replay.

## Recovery für unknown (V1)

- unknown blockt immer — das Artefakt bleibt im Status `blocked`
- V1: Manueller Re-Submit nach Klärung des Grundes
- V2: Automatischer Eskalations-Pfad mit Notification und Override-Mechanismus

## Schema-Migrationen

- Jede Breaking-Änderung = `schema_version++`
- Migrationen sind pure functions: `migrate(artifact@v1) → artifact@v2`
- Event-Log speichert immer die Original-Version
- Schema-Migrationen unterliegen demselben Change-Control-Pfad wie Policy-Änderungen

## Kill-Kriterien

| Kriterium | Grund |
|-----------|-------|
| Migrationen funktionieren nicht sauber | "Alles Artefakte" ist langfristig nicht wartbar |
| Materializer enthält Parsing-Heuristiken | Modell wird wieder zur Autorität |
| Reducer nutzt occurred_at für Logik | Replay ist nicht mehr deterministisch |
