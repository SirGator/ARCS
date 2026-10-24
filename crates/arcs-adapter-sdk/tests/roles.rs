use arcs_adapter_protocol::stdio::{
    AdapterMessage, Envelope, HostMessage, WIRE_VERSION, read_frame, write_frame,
};
use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId,
    CapabilityRef, InputMessage, ReasoningInvocationV2, ReasoningMethod, ReasoningResources,
    ReasoningResponseV2, ReasoningStatus, ReasoningTraceV2, SchemaId,
};
use arcs_adapter_sdk::{Adapter, AdapterError, PendingInput, serve};
use std::io::{BufReader, Cursor};
struct Dual {
    acked: bool,
    calls: usize,
}
impl Adapter for Dual {
    fn manifest(&self) -> AdapterManifest {
        AdapterManifest {
            protocol_version: 1,
            adapter_id: AdapterId("dual".into()),
            adapter_version: "1".into(),
            capabilities: vec![
                CapabilityDescriptor {
                    id: CapabilityId("input".into()),
                    contract: CapabilityContract::Input {
                        emits: vec![SchemaId("arcs.input.sample.v1".into())],
                    },
                    required_permissions: vec![],
                },
                CapabilityDescriptor {
                    id: CapabilityId("reason".into()),
                    contract: CapabilityContract::Reason {
                        emits: vec![SchemaId("arcs.sample_candidate.v1".into())],
                    },
                    required_permissions: vec![],
                },
            ],
        }
    }
    fn poll_input(&mut self) -> Result<Option<PendingInput>, AdapterError> {
        Ok((!self.acked).then(|| PendingInput {
            message_id: "event-1".into(),
            input: InputMessage {
                capability_id: CapabilityId("input".into()),
                external_subject: Some("user".into()),
                external_reference: "event-1".into(),
                payload: serde_json::json!({"task":"test"}),
            },
        }))
    }
    fn acknowledge_input(&mut self, id: &str) -> Result<(), AdapterError> {
        if id != "event-1" {
            return Err(AdapterError::rejected("unknown ACK"));
        }
        self.acked = true;
        Ok(())
    }
    fn reason_v2(
        &mut self,
        request: ReasoningInvocationV2,
    ) -> Result<ReasoningResponseV2, AdapterError> {
        self.calls += 1;
        Ok(ReasoningResponseV2 {
            invocation_id: request.invocation_id,
            request_id: request.request_id,
            status: ReasoningStatus::NoSolution,
            candidates: vec![],
            trace: ReasoningTraceV2 {
                method: ReasoningMethod::Solver,
                implementation: "test".into(),
                version: "1".into(),
                iterations: Some(0),
                output_tokens: None,
                details: serde_json::json!({}),
            },
        })
    }
}
fn invocation() -> ReasoningInvocationV2 {
    ReasoningInvocationV2 {
        invocation_id: "reason:1".into(),
        request_id: "request:1".into(),
        capability: CapabilityRef::new("dual", "reason"),
        method: ReasoningMethod::Solver,
        objective: "test".into(),
        context: vec![],
        input_data: vec![],
        target_schema_id: SchemaId("arcs.sample_candidate.v1".into()),
        allowed_capabilities: vec![],
        constraints: serde_json::json!({}),
        budget: ReasoningResources {
            max_context_items: 1,
            max_context_bytes: 4096,
            max_output_bytes: 4096,
            max_candidates: 1,
            max_runtime_ms: 100,
            max_iterations: Some(1),
            max_output_tokens: None,
        },
    }
}
fn session(adapter: &mut Dual, messages: Vec<HostMessage>) -> Vec<AdapterMessage> {
    let mut input = Vec::new();
    for (index, message) in messages.into_iter().enumerate() {
        write_frame(
            &mut input,
            &Envelope {
                protocol_version: WIRE_VERSION,
                request_id: index.to_string(),
                message,
            },
        )
        .unwrap();
    }
    let mut output = Vec::new();
    serve(
        adapter,
        &mut BufReader::new(Cursor::new(input)),
        &mut output,
    )
    .unwrap();
    let mut reader = BufReader::new(Cursor::new(output));
    let mut results = Vec::new();
    while let Ok(frame) = read_frame::<Envelope<AdapterMessage>>(&mut reader) {
        results.push(frame.message);
    }
    results
}
#[test]
fn one_adapter_process_can_expose_input_and_reason_with_separate_ack_semantics() {
    let mut adapter = Dual {
        acked: false,
        calls: 0,
    };
    let results = session(
        &mut adapter,
        vec![
            HostMessage::Hello,
            HostMessage::PollInput,
            HostMessage::PollInput,
            HostMessage::ReasonV2 {
                invocation: invocation(),
            },
            HostMessage::AckInput {
                message_id: "event-1".into(),
            },
            HostMessage::PollInput,
            HostMessage::Shutdown,
        ],
    );
    assert!(matches!(&results[1],AdapterMessage::Input {message_id,..} if message_id=="event-1"));
    assert!(matches!(&results[2],AdapterMessage::Input {message_id,..} if message_id=="event-1"));
    assert!(matches!(
        &results[3],
        AdapterMessage::ReasoningResultV2 { .. }
    ));
    assert!(matches!(results[5], AdapterMessage::Idle));
    assert_eq!(adapter.calls, 1);
}
#[test]
fn invalid_session_role_and_endpoint_never_reach_the_reasoner() {
    let mut adapter = Dual {
        acked: false,
        calls: 0,
    };
    let mut wrong = invocation();
    wrong.capability.capability_id.0 = "input".into();
    let mut endpoint = invocation();
    endpoint.capability.adapter_id.0 = "other".into();
    let results = session(
        &mut adapter,
        vec![
            HostMessage::ReasonV2 {
                invocation: invocation(),
            },
            HostMessage::Hello,
            HostMessage::ReasonV2 { invocation: wrong },
            HostMessage::ReasonV2 {
                invocation: endpoint,
            },
            HostMessage::Shutdown,
        ],
    );
    assert!(matches!(results[0], AdapterMessage::Error { .. }));
    assert!(matches!(results[2], AdapterMessage::Error { .. }));
    assert!(matches!(results[3], AdapterMessage::Error { .. }));
    assert_eq!(adapter.calls, 0);
}
