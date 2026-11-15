//! Echte Child-Prozesse prüfen erlaubte Variablen ohne Umgebungswerte zu loggen.
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const PEER: &str = r#"import json,os,pathlib,sys
identity=sys.argv[1]; output=pathlib.Path(sys.argv[2])
output.write_text(json.dumps({'a':os.environ.get('ARCS_TEST_KEY_A')=='synthetic-key-a','b':os.environ.get('ARCS_TEST_KEY_B')=='synthetic-key-b','other':'ARCS_TEST_UNRELATED' in os.environ,'option':os.environ.get('ARCS_TEST_OPTION')=='ä value ; $(literal)','path':'PATH' in os.environ,'absent':'ARCS_TEST_ABSENT' in os.environ}))
for line in sys.stdin:
 p=json.loads(line);kind=p['message']['type']
 if kind=='hello': reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':identity,'adapter_version':'1','capabilities':[{'id':'feed','contract':{'kind':'input','emits':['arcs.input.v1']},'required_permissions':[]}]}}
 elif kind=='poll_input': reply={'type':'idle'}
 elif kind=='shutdown': reply={'type':'bye'}
 else: raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown':break
"#;

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "arcs-env-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self { directory };
        std::fs::write(fixture.directory.join("peer.py"), PEER).unwrap();
        let mut adapters = vec![];
        for id in ["a", "b"] {
            fixture.write(&format!("{id}-grant.json"),json!({"adapter_id":id,"producer_class":"adapter","enabled_capabilities":["feed"],"granted_permissions":[],"assigned_trust":"medium","ingress_source_kind":"chat","max_payload_bytes":4096,"max_external_reference_bytes":256,"reasoning_limits":null}));
            adapters.push(json!({"program":"/usr/bin/python3","args":[{"path":"peer.py"},id,{"path":format!("{id}-report.json")}],"grant_file":format!("{id}-grant.json"),"timeout_ms":15000}));
        }
        fixture.write(
            "host.json",
            json!({"config_version":1,"database":"core.sqlite","adapters":adapters}),
        );
        fixture
    }
    fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    fn run(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--config")
            .arg(self.directory.join("host.json"))
            .env("ARCS_TEST_KEY_A", "synthetic-key-a")
            .env("ARCS_TEST_KEY_B", "synthetic-key-b")
            .env("ARCS_TEST_UNRELATED", "synthetic-unrelated-secret")
            .env("ARCS_TEST_OPTION", "ä value ; $(literal)")
            .env_remove("ARCS_TEST_ABSENT")
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn default_environment_excludes_ambient_credentials_for_all_adapters() {
    let f = Fixture::new();
    let result = f.run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for id in ["a", "b"] {
        assert_eq!(
            f.read(&format!("{id}-report.json")),
            json!({"a":false,"b":false,"other":false,"option":false,"path":true,"absent":false})
        );
    }
}

#[test]
fn each_adapter_receives_only_its_declared_environment_and_restarts_safely() {
    let f = Fixture::new();
    let mut host = f.read("host.json");
    host["adapters"][0]["inherit_env"] =
        json!(["ARCS_TEST_KEY_A", "ARCS_TEST_OPTION", "ARCS_TEST_ABSENT"]);
    host["adapters"][1]["inherit_env"] = json!(["ARCS_TEST_KEY_B"]);
    f.write("host.json", host);
    for _ in 0..2 {
        let result = f.run();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            f.read("a-report.json"),
            json!({"a":true,"b":false,"other":false,"option":true,"path":true,"absent":false})
        );
        assert_eq!(
            f.read("b-report.json"),
            json!({"a":false,"b":true,"other":false,"option":false,"path":true,"absent":false})
        );
        let output = String::from_utf8_lossy(&result.stdout);
        assert!(!output.contains("synthetic-key"));
        assert!(!output.contains("synthetic-unrelated-secret"));
    }
    let database = std::fs::read(f.directory.join("core.sqlite")).unwrap();
    assert!(
        !database
            .windows(b"synthetic-key".len())
            .any(|window| window == b"synthetic-key")
    );
}

#[test]
fn invalid_environment_policies_are_rejected_before_any_adapter_starts() {
    for invalid in [
        json!(["DUP", "DUP"]),
        json!(["*"]),
        json!(["NOT=VALID"]),
        json!([""]),
        json!(["3BAD"]),
        json!(["ä"]),
        json!("not-a-list"),
        json!((0..65).map(|i| format!("VAR_{i}")).collect::<Vec<_>>()),
    ] {
        let f = Fixture::new();
        let mut host = f.read("host.json");
        host["adapters"][1]["inherit_env"] = invalid;
        f.write("host.json", host);
        let result = f.run();
        assert!(!result.status.success());
        assert!(!f.directory.join("a-report.json").exists());
        assert!(!f.directory.join("b-report.json").exists());
        assert!(!f.directory.join("core.sqlite").exists());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("synthetic-key"));
    }
}
