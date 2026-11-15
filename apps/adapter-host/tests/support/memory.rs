use arcs::adapters::{AdapterId, AdapterRegistry};
use arcs::core::{Artifact, SystemClock};
use arcs::input::{InputMessage, InputService};
use arcs_host::Host;
use serde_json::{Value, json};

fn adapter(tag: &str) -> String {
    format!("memory.{}", tag.replace(':', "-"))
}
pub fn policy(tag: &str) -> Value {
    json!({"scopes":[{"filter":{"schema_id":"arcs.input.v1","tags":[format!("adapter:{}",adapter(tag))]},"payload_fields":["raw_text"]}],
        "relations":["based_on"],"search":{"max_depth":2,"max_visited":32,"max_edges":64,"max_candidates":8},"max_items":2,"max_bytes":2048})
}
pub fn append(host: &Host, id: &str, tag: &str) -> Artifact {
    let adapter_id = adapter(tag);
    let manifest=serde_json::from_value(json!({"protocol_version":1,"adapter_id":adapter_id,"adapter_version":"1","capabilities":[{"id":"memory.capture","contract":{"kind":"input","emits":["arcs.input.v1"]},"required_permissions":[]}]})).unwrap();
    let grant=serde_json::from_value(json!({"adapter_id":adapter_id,"producer_class":"adapter","enabled_capabilities":["memory.capture"],"granted_permissions":[],"assigned_trust":"medium","ingress_source_kind":"chat","max_payload_bytes":4096,"max_external_reference_bytes":512,"reasoning_limits":null})).unwrap();
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest, grant, host.schemas(), host.store())
        .unwrap();
    let mut ids = host.store().durable_id_generator("memory-fixture").unwrap();
    InputService::new(
        &registry,
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .ingest(
        &AdapterId(adapter_id),
        InputMessage {
            capability_id: arcs::adapters::CapabilityId("memory.capture".into()),
            external_subject: Some(tag.into()),
            external_reference: id.into(),
            payload: json!({"raw_text":format!("remembered evidence: {id}")}),
        },
    )
    .unwrap()
}
pub fn snapshots(host: &Host) -> Vec<Artifact> {
    host.store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .map(|event| event.artifact)
        .filter(|artifact| artifact.schema_id.0 == "arcs.context_selection.v1")
        .collect()
}
