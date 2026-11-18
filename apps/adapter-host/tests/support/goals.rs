//! Gemeinsame echte Goal-Adapter für Ausführungs- und Recovery-Tests.
#![allow(dead_code)]
use arcs_host::Host;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Fixture {
    pub directory: PathBuf,
}
impl Fixture {
    pub fn assessor_peer(&self, mode: &str) {
        let script = r#"import json, sys
for line in sys.stdin:
    packet=json.loads(line); message=packet['message']; kind=message['type']
    if kind=='hello':
        reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'goal.assessor','adapter_version':'0.1.0','capabilities':[{'id':'solution.propose','contract':{'kind':'reason','emits':['arcs.goal_assessment_candidate.v1']},'required_permissions':['reason.compute']}]}}
    elif kind=='reason_v2':
        inv=message['invocation']; goal=inv['constraints']['goal_runtime']; mode=sys.argv[1]
        evidence=list(goal['after'].values())
        if mode=='stale': evidence=list(goal['before'].values())
        if mode=='invented': evidence=['not-an-observation']
        payload={'goal_status':'achieved','effect':'unknown' if mode=='unknown' else 'success','detail':'external test assessment','evidence_version_ids':evidence}
        response={'invocation_id':inv['invocation_id'],'request_id':inv['request_id'],'status':'solved','candidates':[{'schema_id':inv['target_schema_id'],'required_capabilities':[],'referenced_versions':[c['version_id'] for c in inv['context']],'payload':payload}],'trace':{'method':inv['method'],'implementation':'assessment-peer','version':'1','iterations':1,'output_tokens':None,'details':{}}}
        if mode=='no_solution': response['status']='no_solution';response['candidates']=[]
        reply={'type':'reasoning_result_v2','response':response}
    elif kind=='shutdown': reply={'type':'bye'}
    else: raise RuntimeError(kind)
    print(json.dumps({'protocol_version':1,'request_id':packet['request_id'],'message':reply}),flush=True)
    if kind=='shutdown': break
"#;
        std::fs::write(self.directory.join("assessor.py"), script).unwrap();
        let mut config = self.read("host.json");
        config["adapters"][3] = json!({"program":"/usr/bin/python3","args":[{"path":"assessor.py"},mode],"grant_file":"assessor-grant.json","timeout_ms":15000});
        self.write("host.json", config);
    }
    pub fn new() -> Self {
        Self::example("goals")
    }
    pub fn example(example: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-goal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("config")
            .join(example);
        for file in std::fs::read_dir(&source).unwrap() {
            let file = file.unwrap();
            std::fs::copy(file.path(), directory.join(file.file_name())).unwrap();
        }
        let fixture = Self { directory };
        let mut host = fixture.read("host.json");
        host["database"] = json!(fixture.directory.join("core.sqlite"));
        for schema in host["schemas"].as_array_mut().unwrap() {
            *schema = json!(
                source
                    .join(schema.as_str().unwrap())
                    .canonicalize()
                    .unwrap()
            );
        }
        for adapter in host["adapters"].as_array_mut().unwrap() {
            let name = PathBuf::from(adapter["program"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            adapter["program"] = json!(crate::support::adapter_binary(&name));
            let profile = adapter["profile"].as_str().unwrap();
            adapter["args"][0] =
                json!({"path":fixture.directory.join(profile.replace("-profile.json", ".sqlite"))});
        }
        fixture.write("host.json", host);
        let mut runtime = fixture.read("runtime.json");
        // These fixtures exercise evidence, approval and recovery. Concurrent
        // process/SQLite contention must not act as an accidental 1s timeout test.
        // Dedicated stalled-peer tests retain their explicit short deadlines.
        for role in ["planner", "assessor"] {
            let file = format!("{role}-grant.json");
            let mut grant = fixture.read(&file);
            grant["reasoning_limits_v2"]["resources"]["max_runtime_ms"] = json!(15000);
            fixture.write(&file, grant);
        }
        for route in runtime["routes"].as_array_mut().unwrap() {
            route["budget"]["max_runtime_ms"] = json!(15000);
            route["goal"]["assessment"]["budget"]["max_runtime_ms"] = json!(15000);
        }
        runtime["journal"] = json!(fixture.directory.join("runtime.sqlite"));
        runtime["max_jobs_per_tick"] = json!(1);
        runtime["interval_ms"] = json!(1);
        fixture.write("runtime.json", runtime);
        fixture
    }
    pub fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    pub fn text(text: &str) -> Self {
        let fixture = Self::example("text-tasks");
        let mut profile = fixture.read("input-profile.json");
        profile["events"] = json!([{"id":"text-1","subject":"text-1","reference":"task:text-1","payload":{"raw_text":text}}]);
        fixture.write("input-profile.json", profile);
        fixture
    }
    pub fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
    pub fn host(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
    pub fn runtime(&self, host: &Host) -> TaskRuntime {
        TaskRuntime::open(
            host,
            RuntimeConfig::load(self.directory.join("runtime.json")).unwrap(),
        )
        .unwrap()
    }
    pub fn db(&self, name: &str) -> rusqlite::Connection {
        rusqlite::Connection::open(self.directory.join(format!("{name}.sqlite"))).unwrap()
    }
    pub fn manual(&self) {
        let mut config = self.read("runtime.json");
        config["routes"][0]["goal"]["approval"] = json!({"mode":"manual"});
        self.write("runtime.json", config);
    }
    pub fn with_observer(&self, temperature: bool) {
        let mut config = self.read("runtime.json");
        let source = if temperature {
            json!({"id":"ambient","capability":{"adapter_id":"demo.fan","capability_id":"temperature.observe"},"schema_id":"arcs.measurement.temperature.v1","payload_fields":["value","unit"]})
        } else {
            json!({"id":"observed-fan","capability":{"adapter_id":"demo.fan","capability_id":"fan.observe"},"schema_id":"arcs.state.fan.v1","payload_fields":["device_id","enabled","actuation_count"]})
        };
        config["routes"][0]["goal"]["sources"]
            .as_array_mut()
            .unwrap()
            .push(source);
        self.write("runtime.json", config);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

pub fn plan_fixture() -> Fixture {
    let f = Fixture::new();
    f.manual();
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":true,"minimum_actuations":1});
    f.write("input-profile.json", input);
    let mut config = f.read("runtime.json");
    let mut goal = config["routes"][0].clone();
    goal["id"] = json!("switch");
    goal["goal"]["max_rounds"] = json!(2);
    let mut root = goal.clone();
    root.as_object_mut().unwrap().remove("goal");
    root["id"] = json!("plan-task");
    root["reasoner"]["adapter_id"] = json!("planner");
    root["method"] = json!("learned");
    root["target_schema_id"] = json!("arcs.plan_candidate.v1");
    root["allowed_capabilities"] = json!([]);
    root["budget"]["max_runtime_ms"] = json!(10000);
    root["plan"] = json!({"max_steps":3,"max_total_runtime_ms":120000,"max_total_actions":4,"templates":[{"route":goal,"export_fields":["goal_status","effect"]}]});
    config["routes"] = json!([root]);
    config["max_concurrent_reasoning"] = json!(2);
    config["max_jobs_per_tick"] = json!(10);
    config["max_pending_jobs"] = json!(4);
    f.write("runtime.json", config);
    f.write(
        "planner.json",
        json!({"proposal":{"steps":[{"id":"device","template":"switch"}]}}),
    );
    std::fs::write(
        f.directory.join("planner.py"),
        include_str!("planner_peer.py"),
    )
    .unwrap();
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("planner");
    grant["producer_class"] = json!("model");
    grant["reasoning_limits_v2"]["methods"] = json!(["learned"]);
    f.write("root-grant.json", grant);
    let mut host = f.read("host.json");
    host["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"planner.py"},{"path":"planner.json"}],"grant_file":"root-grant.json","timeout_ms":15000}));
    f.write("host.json", host);
    f
}
