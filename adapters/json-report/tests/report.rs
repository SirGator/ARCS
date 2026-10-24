use arcs_adapter_protocol::{CapabilityRef, SchemaId, TransformInvocation, VersionId};
use arcs_adapter_sdk::Adapter;
use arcs_json_report::{JsonReport, Profile};
use serde_json::{Value, json};

fn adapter() -> JsonReport {
    JsonReport::new(Profile {
        adapter_id: arcs_adapter_protocol::AdapterId("test.report".into()),
        accepts: vec![
            SchemaId("test.measurement.v1".into()),
            SchemaId("test.inventory.v1".into()),
        ],
    })
    .unwrap()
}

fn invocation(schema: &str, payload: Value) -> TransformInvocation {
    TransformInvocation {
        invocation_id: "transform:test-1".into(),
        capability: CapabilityRef::new("test.report", "json.report"),
        input_version_id: VersionId("evidence-1".into()),
        input_schema_id: SchemaId(schema.into()),
        input_payload: payload,
        output_schema_id: SchemaId("arcs.report.v1".into()),
        max_output_bytes: 4096,
    }
}

#[test]
fn unrelated_contracts_keep_their_exact_payload_and_evidence_reference() {
    let mut adapter = adapter();
    for (schema, payload) in [
        ("test.measurement.v1", json!({"value":26.0,"unit":"Cel"})),
        (
            "test.inventory.v1",
            json!({"items":[{"id":"part-1","available":false}],"note":"<untrusted>\nß"}),
        ),
    ] {
        let call = invocation(schema, payload.clone());
        let response = adapter.transform(call.clone()).unwrap();
        assert_eq!(response.invocation_id, call.invocation_id);
        let body: Value = serde_json::from_str(response.payload["body"].as_str().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"schema_id":schema,"version_id":"evidence-1","payload":payload})
        );
        assert_eq!(adapter.transform(call).unwrap(), response);
    }
}

#[test]
fn undeclared_contracts_and_exceeded_limits_are_rejected() {
    let mut adapter = adapter();
    let call = invocation("test.measurement.v1", json!({"value":1}));
    let mut wrong = call.clone();
    wrong.capability = CapabilityRef::new("test.report", "act");
    assert!(adapter.transform(wrong).is_err());
    let mut wrong = call.clone();
    wrong.input_schema_id = SchemaId("unknown.v1".into());
    assert!(adapter.transform(wrong).is_err());
    let mut too_small = call.clone();
    too_small.max_output_bytes = 1;
    assert!(adapter.transform(too_small).is_err());
    let mut too_large = call;
    too_large.max_output_bytes = 1024 * 1024;
    too_large.input_payload = json!({"text":"x".repeat(60 * 1024)});
    assert!(adapter.transform(too_large).is_err());
}

#[test]
fn profiles_need_explicit_nonempty_unique_contracts() {
    for accepts in [
        vec![],
        vec![SchemaId("test.v1".into()), SchemaId("test.v1".into())],
    ] {
        assert!(
            JsonReport::new(Profile {
                adapter_id: arcs_adapter_protocol::AdapterId("test.report".into()),
                accepts
            })
            .is_err()
        );
    }
}

#[test]
fn compact_preserves_json_content_and_title_and_rejects_non_json_reports() {
    let mut adapter = adapter();
    let original = json!({"items":[1,2],"note":"untrusted\ntext"});
    let body = serde_json::to_string_pretty(&original).unwrap();
    let mut call = invocation("arcs.report.v1", json!({"title":"A report","body":body}));
    call.capability.capability_id.0 = "report.compact".into();
    let result = adapter.transform(call.clone()).unwrap();
    assert_eq!(result.payload["title"], "A report");
    let compact = result.payload["body"].as_str().unwrap();
    assert!(compact.len() < body.len());
    assert_eq!(serde_json::from_str::<Value>(compact).unwrap(), original);
    call.input_payload = json!({"title":"Plain text","body":"not JSON"});
    assert!(adapter.transform(call).is_err());
}
