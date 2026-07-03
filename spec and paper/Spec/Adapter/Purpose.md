# Adapter

## Purpose

Adapters are the boundary layer between ARCS and the external world.

Their purpose is to translate external signals into ARCS-compatible ingress artifacts and to translate final ARCS outputs into formats suitable for external consumers.

Adapters MUST NOT decide what ARCS should do. They MUST NOT plan, verify, approve, materialize, execute, or create authoritative internal decisions. Their responsibility is limited to boundary translation, source attribution, input normalization, basic ingress constraint enforcement, and output formatting.

An Adapter MAY receive input from:

* user interfaces,
* API requests,
* file events,
* scheduled tasks,
* system signals,
* webhooks,
* tool callbacks,
* watcher events,
* sensor events,
* platform events,
* external services.

Regardless of source, external input MUST NOT enter the Core as raw unstructured authority. The Adapter MUST normalize the external signal into an `ingress_event` draft or an equivalent schema-valid ingress artifact accepted by the Core.

The Adapter protects the Core from external format inconsistency. The Core SHOULD NOT need to understand raw user messages, HTTP payloads, UI events, file-system events, webhook formats, platform-specific event structures, or external protocol details. These concerns belong to the Adapter layer.

An Adapter is responsible for:

* receiving external input,
* extracting source metadata,
* extracting actor or identity metadata if available,
* preserving raw payload references when required,
* normalizing external data into ARCS-compatible structure,
* attaching channel, trust, and context metadata,
* enforcing basic input constraints,
* creating an initial `ingress_event` draft,
* submitting the ingress artifact to the Core,
* receiving final internal output from the Core or Output Coordinator,
* converting final system output into an external target format.

An Adapter MUST NOT create authoritative system state by itself. Any state-relevant information produced by an Adapter becomes authoritative only after schema validation and Store commit through the Core lifecycle.

An Adapter MUST NOT bypass:

* Schema Gate,
* Store commit,
* Reducers,
* Verification,
* Approval when required by policy,
* Action Materialization,
* Action Verification,
* Execution control,
* Event logging,
* Audit and replay requirements.

Adapters MAY perform basic syntactic validation and input normalization before submitting data to the Core. However, Adapter-level validation MUST NOT replace Core schema validation, policy validation, verification, or permission checks.

Adapters MAY reject malformed external input before it enters ARCS. If rejection is relevant for audit, security, rate limiting, or incident analysis, the rejection SHOULD be represented through an explicit event, log entry, or ingress rejection artifact according to policy.

For output, an Adapter MAY format, filter, summarize, serialize, translate, or route final ARCS output to an external target. Output formatting MUST NOT create new authoritative state and MUST NOT hide safety-relevant failure states unless policy explicitly permits redaction.

The Adapter layer exists to make external systems compatible with ARCS without allowing external systems to control ARCS directly.

In short, Adapters translate at the boundary. The Core governs the lifecycle.
