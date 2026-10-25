use serde_json::json;
use std::process::Command;

#[test]
fn input_preflight_uses_the_core_contract_without_a_host_or_adapter_session() {
    let directory = std::env::temp_dir().join(format!("arcs-input-check-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("check.json");
    for (payload, valid) in [
        (json!({"count":1}), true),
        (json!({"count":1.0}), false),
        (json!({}), false),
        (json!({"count":1,"extra":true}), false),
    ] {
        std::fs::write(&path,serde_json::to_vec(&json!({"schema":{"$id":"arcs.input.check.v1","type":"object","required":["count"],"properties":{"count":{"type":"integer","minimum":1}},"additionalProperties":false},"payload":payload})).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--validate-input")
            .arg(&path)
            .current_dir(&directory)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            valid,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if valid {
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                json!({"valid":true})
            );
        }
    }
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}
