use std::io::Cursor;

use arcs_adapter_protocol::{
    ActInvocation, ActResponse, AdapterId, AdapterManifest, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ObservationMessage, RequestInvocation,
    RequestResponse, SchemaId, SubjectId, VersionId,
    stdio::{
        AdapterMessage, Envelope, HostMessage, WIRE_VERSION, WireError, read_frame, write_frame,
    },
};
use arcs_adapter_sdk::{Adapter, AdapterError, PendingObservation, ServerError, serve};
use serde_json::json;

fn schema(id: &str) -> SchemaId {
    SchemaId(id.into())
}

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: 1,
        adapter_id: AdapterId("test.device".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![
            CapabilityDescriptor {
                id: CapabilityId("observe".into()),
                contract: CapabilityContract::Observe {
                    emits: vec![schema("state")],
                },
                required_permissions: vec![],
            },
            CapabilityDescriptor {
                id: CapabilityId("read".into()),
                contract: CapabilityContract::Request {
                    accepts: vec![schema("query")],
                    emits: vec![schema("state")],
                },
                required_permissions: vec![],
            },
            CapabilityDescriptor {
                id: CapabilityId("set".into()),
                contract: CapabilityContract::Act {
                    accepts: vec![schema("command")],
                    emits: vec![schema("result")],
                    idempotent: true,
                },
                required_permissions: vec!["device.write".into()],
            },
        ],
    }
}

#[derive(Default)]
struct Device {
    polls: usize,
    acks: usize,
    requests: usize,
    executions: usize,
    data_reads: usize,
    acknowledged: bool,
    wrong_response: bool,
    failure: bool,
}

impl Adapter for Device {
    fn read_data(
        &mut self,
        request: arcs_adapter_protocol::DataRead,
    ) -> Result<arcs_adapter_protocol::DataChunk, AdapterError> {
        self.data_reads += 1;
        Ok(arcs_adapter_protocol::DataChunk {
            reference_id: if self.wrong_response {
                "other".into()
            } else {
                request.reference.id.clone()
            },
            offset: request.offset,
            bytes: vec![255; request.expected_bytes().unwrap()],
        })
    }
    fn manifest(&self) -> AdapterManifest {
        manifest()
    }

    fn poll(&mut self) -> Result<Option<PendingObservation>, AdapterError> {
        self.polls += 1;
        if self.failure {
            return Err(AdapterError::unavailable("device disconnected"));
        }
        Ok((!self.acknowledged).then(|| PendingObservation {
            message_id: "event-1".into(),
            observation: ObservationMessage {
                capability_id: CapabilityId(
                    if self.wrong_response {
                        "set"
                    } else {
                        "observe"
                    }
                    .into(),
                ),
                external_subject: Some("device-1".into()),
                external_reference: "sample-1".into(),
                payload: json!({"value": 1}),
            },
        }))
    }

    fn acknowledge(&mut self, message_id: &str) -> Result<(), AdapterError> {
        self.acks += 1;
        if message_id != "event-1" {
            return Err(AdapterError::rejected("unknown acknowledgement"));
        }
        self.acknowledged = true;
        Ok(())
    }

    fn request(&mut self, invocation: RequestInvocation) -> Result<RequestResponse, AdapterError> {
        self.requests += 1;
        Ok(RequestResponse {
            invocation_id: if self.wrong_response {
                "wrong".into()
            } else {
                invocation.invocation_id
            },
            external_reference: "read-1".into(),
            payload: json!({"value": 1}),
        })
    }

    fn execute(&mut self, invocation: ActInvocation) -> Result<ActResponse, AdapterError> {
        self.executions += 1;
        Ok(ActResponse {
            invocation_id: if self.wrong_response {
                "wrong".into()
            } else {
                invocation.invocation_id
            },
            external_reference: "effect-1".into(),
            result_payload: json!({"applied": true}),
        })
    }
}

fn query() -> RequestInvocation {
    RequestInvocation {
        invocation_id: "read-1".into(),
        capability: CapabilityRef::new("test.device", "read"),
        request_version_id: VersionId("query-v1".into()),
        request_schema_id: schema("query"),
        subject: SubjectId("device-1".into()),
        request_payload: json!({}),
        response_schema_id: schema("state"),
        max_response_bytes: 1024,
    }
}

fn action() -> ActInvocation {
    ActInvocation {
        invocation_id: "act-1".into(),
        capability: CapabilityRef::new("test.device", "set"),
        action_version_id: VersionId("action-v1".into()),
        action_schema_id: schema("action"),
        payload: json!({}),
        result_schema_id: schema("result"),
    }
}

fn frames(messages: Vec<HostMessage>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, message) in messages.into_iter().enumerate() {
        write_frame(
            &mut bytes,
            &Envelope {
                protocol_version: WIRE_VERSION,
                request_id: format!("wire-{index}"),
                message,
            },
        )
        .unwrap();
    }
    bytes
}

fn session(adapter: &mut impl Adapter, messages: Vec<HostMessage>) -> Vec<AdapterMessage> {
    let count = messages.len();
    let mut output = Vec::new();
    serve(adapter, &mut Cursor::new(frames(messages)), &mut output).unwrap();
    let mut reader = Cursor::new(output);
    let messages = (0..count)
        .map(|index| {
            let envelope: Envelope<AdapterMessage> = read_frame(&mut reader).unwrap();
            assert_eq!(envelope.protocol_version, WIRE_VERSION);
            assert_eq!(envelope.request_id, format!("wire-{index}"));
            envelope.message
        })
        .collect();
    assert_eq!(reader.position() as usize, reader.get_ref().len());
    messages
}

fn assert_error(message: &AdapterMessage, expected: &str) {
    assert!(
        matches!(message, AdapterMessage::Error { code, .. } if code == expected),
        "{message:?}"
    );
}

#[test]
fn shared_session_supports_observation_ack_request_and_action() {
    let mut device = Device::default();
    let messages = session(
        &mut device,
        vec![
            HostMessage::Hello,
            HostMessage::Poll,
            HostMessage::Poll,
            HostMessage::Ack {
                message_id: "event-1".into(),
            },
            HostMessage::Ack {
                message_id: "event-1".into(),
            },
            HostMessage::Poll,
            HostMessage::Request {
                invocation: query(),
            },
            HostMessage::Execute {
                invocation: action(),
            },
            HostMessage::Shutdown,
        ],
    );
    assert!(
        matches!(&messages[0], AdapterMessage::Manifest { manifest: found } if found == &manifest())
    );
    for message in &messages[1..3] {
        assert!(
            matches!(message, AdapterMessage::Observation { message_id, .. } if message_id == "event-1")
        );
    }
    assert!(matches!(messages[3], AdapterMessage::Acknowledged { .. }));
    assert!(matches!(messages[4], AdapterMessage::Acknowledged { .. }));
    assert!(matches!(messages[5], AdapterMessage::Idle));
    assert!(
        matches!(&messages[6], AdapterMessage::RequestResult { response } if response.invocation_id == "read-1")
    );
    assert!(
        matches!(&messages[7], AdapterMessage::ExecutionResult { response } if response.invocation_id == "act-1")
    );
    assert!(matches!(messages[8], AdapterMessage::Bye));
    assert_eq!(
        (
            device.polls,
            device.acks,
            device.requests,
            device.executions
        ),
        (3, 2, 1, 1)
    );
}

#[test]
fn operations_before_hello_do_not_reach_the_device() {
    let mut device = Device::default();
    let messages = session(
        &mut device,
        vec![
            HostMessage::Poll,
            HostMessage::Ack {
                message_id: "event-1".into(),
            },
            HostMessage::Request {
                invocation: query(),
            },
            HostMessage::Execute {
                invocation: action(),
            },
            HostMessage::Shutdown,
        ],
    );
    for message in &messages[..4] {
        assert_error(message, "invalid_state");
    }
    assert_eq!(
        (
            device.polls,
            device.acks,
            device.requests,
            device.executions
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn incorrect_routing_and_schema_contracts_do_not_invoke_device_operations() {
    let mut messages = vec![HostMessage::Hello];
    for change in 0..6 {
        let mut invocation = query();
        match change {
            0 => invocation.capability.adapter_id.0 = "another.device".into(),
            1 => invocation.capability.capability_id.0 = "set".into(),
            2 => invocation.capability.capability_id.0 = "missing".into(),
            3 => invocation.request_schema_id = schema("other"),
            4 => invocation.response_schema_id = schema("other"),
            _ => invocation.invocation_id.clear(),
        }
        messages.push(HostMessage::Request { invocation });
    }
    for change in 0..5 {
        let mut invocation = action();
        match change {
            0 => invocation.capability.adapter_id.0 = "another.device".into(),
            1 => invocation.capability.capability_id.0 = "read".into(),
            2 => invocation.result_schema_id = schema("other"),
            3 => invocation.invocation_id.clear(),
            _ => invocation.action_version_id.0.clear(),
        }
        messages.push(HostMessage::Execute { invocation });
    }
    messages.push(HostMessage::Shutdown);
    let mut device = Device::default();
    let responses = session(&mut device, messages);
    for response in &responses[1..12] {
        assert_error(response, "invalid_request");
    }
    assert_eq!((device.requests, device.executions), (0, 0));
}

#[test]
fn callback_responses_must_match_the_invocation_and_observation_contract() {
    let mut device = Device {
        wrong_response: true,
        ..Device::default()
    };
    let messages = session(
        &mut device,
        vec![
            HostMessage::Hello,
            HostMessage::Poll,
            HostMessage::Request {
                invocation: query(),
            },
            HostMessage::Execute {
                invocation: action(),
            },
            HostMessage::Shutdown,
        ],
    );
    for message in &messages[1..4] {
        assert_error(message, "invalid_response");
    }
}

#[test]
fn request_payload_limit_is_enforced_on_callback_results() {
    let mut invocation = query();
    invocation.max_response_bytes = 1;
    let messages = session(
        &mut Device::default(),
        vec![
            HostMessage::Hello,
            HostMessage::Request { invocation },
            HostMessage::Shutdown,
        ],
    );
    assert_error(&messages[1], "invalid_response");
}

#[test]
fn callback_errors_preserve_retry_information() {
    let mut device = Device {
        failure: true,
        ..Device::default()
    };
    let messages = session(
        &mut device,
        vec![HostMessage::Hello, HostMessage::Poll, HostMessage::Shutdown],
    );
    assert!(
        matches!(&messages[1], AdapterMessage::Error { code, message, retryable: true } if code == "unavailable" && message == "device disconnected")
    );
}

#[test]
fn version_and_repeated_hello_errors_do_not_reset_the_session() {
    let mut input = Vec::new();
    write_frame(
        &mut input,
        &Envelope {
            protocol_version: 99,
            request_id: "bad-version".into(),
            message: HostMessage::Shutdown,
        },
    )
    .unwrap();
    input.extend(frames(vec![
        HostMessage::Hello,
        HostMessage::Hello,
        HostMessage::Poll,
        HostMessage::Shutdown,
    ]));
    let mut output = Vec::new();
    serve(&mut Device::default(), &mut Cursor::new(input), &mut output).unwrap();
    let mut reader = Cursor::new(output);
    let first: Envelope<AdapterMessage> = read_frame(&mut reader).unwrap();
    assert_eq!(first.request_id, "bad-version");
    assert_error(&first.message, "unsupported_version");
    let _: Envelope<AdapterMessage> = read_frame(&mut reader).unwrap();
    let repeated: Envelope<AdapterMessage> = read_frame(&mut reader).unwrap();
    assert_error(&repeated.message, "invalid_state");
    let polled: Envelope<AdapterMessage> = read_frame(&mut reader).unwrap();
    assert!(matches!(polled.message, AdapterMessage::Observation { .. }));
}

#[test]
fn invalid_manifest_fails_before_any_bytes_are_written() {
    struct Invalid;
    impl Adapter for Invalid {
        fn manifest(&self) -> AdapterManifest {
            let mut manifest = manifest();
            manifest.capabilities.clear();
            manifest
        }
    }
    let mut output = Vec::new();
    let result = serve(
        &mut Invalid,
        &mut Cursor::new(frames(vec![HostMessage::Hello])),
        &mut output,
    );
    assert!(matches!(result, Err(ServerError::Manifest(_))));
    assert!(output.is_empty());
}

#[test]
fn incomplete_and_oversized_frames_fail_before_device_calls() {
    for input in [b"{\"protocol_version\":1}".to_vec(), vec![b'x'; 65_537]] {
        let mut device = Device::default();
        let mut output = Vec::new();
        assert!(matches!(
            serve(&mut device, &mut Cursor::new(input), &mut output),
            Err(ServerError::Wire(
                WireError::Disconnected | WireError::FrameTooLarge
            ))
        ));
        assert!(output.is_empty());
        assert_eq!(
            (device.polls, device.requests, device.executions),
            (0, 0, 0)
        );
    }
}

#[test]
fn unimplemented_callbacks_report_unsupported_operation() {
    struct Unimplemented;
    impl Adapter for Unimplemented {
        fn manifest(&self) -> AdapterManifest {
            manifest()
        }
    }
    let messages = session(
        &mut Unimplemented,
        vec![
            HostMessage::Hello,
            HostMessage::Request {
                invocation: query(),
            },
            HostMessage::Execute {
                invocation: action(),
            },
            HostMessage::Shutdown,
        ],
    );
    assert_error(&messages[1], "unsupported_operation");
    assert_error(&messages[2], "unsupported_operation");
}

fn data_request() -> arcs_adapter_protocol::DataRead {
    arcs_adapter_protocol::DataRead {
        capability: CapabilityRef::new("test.device", "observe"),
        reference: arcs_adapter_protocol::DataReference {
            id: "blob-1".into(),
            media_type: "image/png".into(),
            size_bytes: 100_000,
        },
        offset: 8192,
        max_bytes: 8192,
    }
}

#[test]
fn sdk_serves_bounded_data_chunks_and_checks_callback_correlation() {
    let mut device = Device::default();
    let messages = session(
        &mut device,
        vec![
            HostMessage::Hello,
            HostMessage::ReadData {
                request: data_request(),
            },
            HostMessage::Shutdown,
        ],
    );
    assert!(
        matches!(&messages[1], AdapterMessage::DataChunk { chunk } if chunk.bytes.len() == 8192 && chunk.offset == 8192)
    );
    assert_eq!(device.data_reads, 1);
    device.wrong_response = true;
    let messages = session(
        &mut device,
        vec![
            HostMessage::Hello,
            HostMessage::ReadData {
                request: data_request(),
            },
            HostMessage::Shutdown,
        ],
    );
    assert_error(&messages[1], "invalid_response");
}

#[test]
fn invalid_data_requests_do_not_reach_the_driver() {
    let mut messages = vec![
        HostMessage::ReadData {
            request: data_request(),
        },
        HostMessage::Hello,
    ];
    for change in 0..6 {
        let mut request = data_request();
        match change {
            0 => request.capability.adapter_id.0 = "other".into(),
            1 => request.capability.capability_id.0 = "set".into(),
            2 => request.reference.id = "../secret".into(),
            3 => request.offset = request.reference.size_bytes,
            4 => request.max_bytes = 0,
            _ => request.max_bytes = 8193,
        }
        messages.push(HostMessage::ReadData { request });
    }
    messages.push(HostMessage::Shutdown);
    let mut device = Device::default();
    let messages = session(&mut device, messages);
    assert_error(&messages[0], "invalid_state");
    for message in &messages[2..8] {
        assert_error(message, "invalid_request");
    }
    assert_eq!(device.data_reads, 0);
}
