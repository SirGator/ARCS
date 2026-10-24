use arcs_adapter_protocol::stdio::*;
use arcs_adapter_protocol::*;
use arcs_adapter_sdk::{Adapter, AdapterError, serve};
use std::io::{BufReader, Cursor};
struct Receiver {
    calls: usize,
    received: Vec<u8>,
}
impl Adapter for Receiver {
    fn manifest(&self) -> AdapterManifest {
        AdapterManifest {
            protocol_version: 1,
            adapter_id: AdapterId("receiver".into()),
            adapter_version: "1".into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("reason".into()),
                contract: CapabilityContract::Reason {
                    emits: vec![SchemaId("arcs.result_candidate.v1".into())],
                },
                required_permissions: vec![],
            }],
        }
    }
    fn reason_with_data(
        &mut self,
        invocation: ReasoningInvocationV2,
        inputs: Vec<ReceivedInputData>,
    ) -> Result<ReasoningResponseV2, AdapterError> {
        self.calls += 1;
        self.received = inputs.into_iter().flat_map(|input| input.bytes).collect();
        Ok(ReasoningResponseV2 {
            invocation_id: invocation.invocation_id,
            request_id: invocation.request_id,
            status: ReasoningStatus::NoSolution,
            candidates: vec![],
            trace: ReasoningTraceV2 {
                method: ReasoningMethod::Rules,
                implementation: "test".into(),
                version: "1".into(),
                iterations: Some(0),
                output_tokens: None,
                details: serde_json::json!({}),
            },
        })
    }
}
fn descriptor() -> InputDataDescriptor {
    InputDataDescriptor {
        selection: InputDataSelection {
            name: "image".into(),
            source: CapabilityRef::new("camera", "capture"),
            source_version_id: VersionId("frame-1".into()),
            reference_pointer: "/image".into(),
        },
        reference: DataReference {
            id: "image-1".into(),
            media_type: "application/octet-stream".into(),
            size_bytes: 3,
        },
    }
}
fn invocation() -> ReasoningInvocationV2 {
    let input = descriptor();
    ReasoningInvocationV2 {
        invocation_id: "job-1".into(),
        request_id: "request-1".into(),
        capability: CapabilityRef::new("receiver", "reason"),
        method: ReasoningMethod::Rules,
        objective: "test".into(),
        context: vec![ReasoningContextItem {
            version_id: VersionId("frame-1".into()),
            schema_id: SchemaId("arcs.data.frame.v1".into()),
            artifact_type: "data".into(),
            trust_level: TrustLevel::Low,
            payload: serde_json::json!({"image":input.reference}),
        }],
        input_data: vec![input],
        target_schema_id: SchemaId("arcs.result_candidate.v1".into()),
        allowed_capabilities: vec![],
        constraints: serde_json::json!({}),
        budget: ReasoningResources {
            max_context_items: 1,
            max_context_bytes: 4096,
            max_output_bytes: 4096,
            max_candidates: 1,
            max_runtime_ms: 1000,
            max_iterations: Some(1),
            max_output_tokens: None,
        },
    }
}
fn begin() -> HostMessage {
    HostMessage::BeginInputData {
        invocation_id: "job-1".into(),
        target: CapabilityRef::new("receiver", "reason"),
        inputs: vec![descriptor()],
    }
}
fn chunk(offset: u64, bytes: Vec<u8>) -> HostMessage {
    HostMessage::InputDataChunk {
        invocation_id: "job-1".into(),
        name: "image".into(),
        offset,
        bytes,
    }
}
fn run(messages: Vec<HostMessage>) -> (Receiver, Vec<AdapterMessage>) {
    let mut input = Vec::new();
    for (i, message) in messages.into_iter().enumerate() {
        write_frame(
            &mut input,
            &Envelope {
                protocol_version: WIRE_VERSION,
                request_id: i.to_string(),
                message,
            },
        )
        .unwrap();
    }
    let mut receiver = Receiver {
        calls: 0,
        received: vec![],
    };
    let mut output = Vec::new();
    serve(
        &mut receiver,
        &mut BufReader::new(Cursor::new(input)),
        &mut output,
    )
    .unwrap();
    let messages = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<Envelope<AdapterMessage>>(line)
                .unwrap()
                .message
        })
        .collect();
    (receiver, messages)
}
#[test]
fn complete_inputs_are_consumed_once_and_bound_to_the_invocation() {
    let (receiver, messages) = run(vec![
        HostMessage::Hello,
        begin(),
        chunk(0, vec![1, 2]),
        chunk(2, vec![3]),
        HostMessage::ReasonV2 {
            invocation: invocation(),
        },
        HostMessage::ReasonV2 {
            invocation: invocation(),
        },
        HostMessage::Shutdown,
    ]);
    assert_eq!(receiver.calls, 1);
    assert_eq!(receiver.received, [1, 2, 3]);
    assert!(matches!(
        messages[4],
        AdapterMessage::ReasoningResultV2 { .. }
    ));
    assert!(matches!(messages[5], AdapterMessage::Error { .. }));
}
#[test]
fn missing_chunks_wrong_offsets_and_unselected_references_never_reach_the_backend() {
    for kind in 0..4 {
        let mut task = invocation();
        let mut messages = vec![HostMessage::Hello, begin()];
        match kind {
            0 => messages.push(chunk(1, vec![1, 2, 3])),
            1 => messages.push(chunk(0, vec![1, 2])),
            2 => {
                messages.push(chunk(0, vec![1, 2, 3]));
                task.context.clear();
            }
            _ => {
                messages.push(chunk(0, vec![1, 2, 3]));
                task.invocation_id = "other".into();
            }
        }
        messages.push(HostMessage::ReasonV2 { invocation: task });
        messages.push(HostMessage::Shutdown);
        let (receiver, _) = run(messages);
        assert_eq!(receiver.calls, 0);
    }
}
#[test]
fn duplicate_names_oversized_batches_and_before_hello_uploads_are_rejected() {
    let mut oversized = descriptor();
    oversized.reference.size_bytes = MAX_INPUT_DATA_BYTES + 1;
    let (receiver, messages) = run(vec![
        begin(),
        HostMessage::Hello,
        HostMessage::BeginInputData {
            invocation_id: "job-1".into(),
            target: CapabilityRef::new("receiver", "reason"),
            inputs: vec![descriptor(), descriptor()],
        },
        HostMessage::BeginInputData {
            invocation_id: "job-1".into(),
            target: CapabilityRef::new("receiver", "reason"),
            inputs: vec![oversized],
        },
        HostMessage::Shutdown,
    ]);
    assert_eq!(receiver.calls, 0);
    for i in [0, 2, 3] {
        assert!(matches!(messages[i], AdapterMessage::Error { .. }));
    }
}
