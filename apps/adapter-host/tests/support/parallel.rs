//! Gemeinsame externe Reasoner mit Dateigates und persistenten Replay-Belegen.
#![allow(dead_code)]
use arcs_host::Host;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// File gates establish ordering without relying on a model's running time.
// Each peer journals results before replying, reproducing a lost-response crash.
const PEER: &str = r#"import json, pathlib, sqlite3, sys, time
c=json.load(open(sys.argv[1])); root=pathlib.Path(sys.argv[1]).parent
db=sqlite3.connect(root/(c['id']+'.sqlite'))
db.executescript('CREATE TABLE IF NOT EXISTS results(id TEXT PRIMARY KEY,input TEXT,response TEXT); CREATE TABLE IF NOT EXISTS attempts(id TEXT);')
n=0
for line in sys.stdin:
 p=json.loads(line); m=p['message']; kind=m['type']
 if kind=='hello':
  caps=[{'id':'solution.propose','contract':{'kind':'reason','emits':['arcs.solution_candidate.nearest.v1']},'required_permissions':['reason.compute']}]
  if c.get('observe'): caps.append({'id':'status','contract':{'kind':'observe','emits':['arcs.task.nearest.v1']},'required_permissions':['observe.read']})
  reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':c['id'],'adapter_version':'1','capabilities':caps}}
 elif kind=='reason_v2':
  inv=m['invocation']; key=inv['invocation_id']; raw=json.dumps(inv,sort_keys=True)
  with db: db.execute('INSERT INTO attempts VALUES(?)',(key,))
  if c.get('mode')=='fail':
   reply={'type':'error','code':'unavailable','message':c.get('failure_message','injected component failure'),'retryable':True}
  else:
   old=db.execute('SELECT input,response FROM results WHERE id=?',(key,)).fetchone()
   if old:
    assert old[0]==raw, 'invocation changed during recovery'
    reply=json.loads(old[1])
   else:
    task=next(item['payload'] for item in inv['context'] if item['schema_id']=='arcs.task.nearest.v1'); v=min(task['values'],key=lambda v:abs(v-task['target']))
    payload={'value':v,'distance':abs(v-task['target'])}
    if c.get('mode')=='invalid': payload={'untrusted':'wrong schema'}
    if c.get('mode')=='join':
     parents=[i['payload'] for i in inv['context'] if i['schema_id']==c.get('parent_schema_id','arcs.solution_candidate.nearest.v1')]
     assert parents and all(set(p)=={'value'} for p in parents),'unexpected dependency payload fields'
     payload={'value':sum(p['value'] for p in parents),'distance':0}
    reply={'type':'reasoning_result_v2','response':{'invocation_id':key,'request_id':inv['request_id'],'status':'solved','candidates':[{'schema_id':inv['target_schema_id'],'required_capabilities':[],'referenced_versions':[i['version_id'] for i in inv['context']],'payload':payload}],'trace':{'method':inv['method'],'implementation':'parallel-test-peer','version':'1','iterations':None,'output_tokens':None,'details':{}}}}
    if c.get('mode')=='no_solution': reply['response']['status']='no_solution';reply['response']['candidates']=[]
    if c.get('mode')=='many': reply['response']['candidates']*=2
    with db: db.execute('INSERT INTO results VALUES(?,?,?)',(key,raw,json.dumps(reply)))
   (root/(c['id']+'.started')).write_text(key)
   while c.get('mode')=='gate' and not (root/(c['id']+'.release')).exists(): time.sleep(.005)
 elif kind=='poll':
  n+=1
  reply={'type':'observation','message_id':str(n),'observation':{'capability_id':'status','external_subject':'status','external_reference':str(n),'payload':{'values':[n],'target':n}}}
 elif kind=='ack': reply={'type':'acknowledged','message_id':m['message_id']}
 elif kind=='shutdown': reply={'type':'bye'}
 else: raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown': break
"#;

pub struct Fixture {
    pub directory: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "arcs-parallel-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self { directory };
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in ["input-profile.json", "input-grant.json"] {
            std::fs::copy(
                root.join("apps/adapter-host/config/roles").join(name),
                fixture.directory.join(name),
            )
            .unwrap();
        }
        std::fs::write(fixture.directory.join("peer.py"), PEER).unwrap();
        fixture.write("host.json", json!({"config_version":1,"database":"core.sqlite","schemas":[root.join("adapters/example-reasoner/schemas/task.schema.json"),root.join("adapters/example-reasoner/schemas/candidate.schema.json")],"polling":{"max_observations_per_adapter":1},"adapters":[{"program":crate::support::adapter_binary("arcs-json-input"),"profile":"input-profile.json","args":[{"path":"input.sqlite"}],"grant_file":"input-grant.json","timeout_ms":15000}]}));
        fixture.write("runtime.json", json!({"config_version":1,"runtime_id":"parallel-test","journal":"runtime.sqlite","max_concurrent_reasoning":2,"max_jobs_per_tick":10,"max_pending_jobs":16,"routes":[]}));
        fixture
    }
    pub fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    pub fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    pub fn peer(&self, id: &str, mode: &str, observe: bool) {
        self.write(
            &format!("{id}.json"),
            json!({"id":id,"mode":mode,"observe":observe}),
        );
        self.write(&format!("{id}-grant.json"), json!({"adapter_id":id,"producer_class":"model","enabled_capabilities":if observe {vec!["solution.propose","status"]} else {vec!["solution.propose"]},"granted_permissions":if observe {vec!["reason.compute","observe.read"]} else {vec!["reason.compute"]},"assigned_trust":"medium","ingress_source_kind":if observe {Some("sensor")} else {None},"max_payload_bytes":4096,"max_external_reference_bytes":256,"reasoning_limits":null,"reasoning_limits_v2":{"methods":["learned"],"resources":{"max_context_items":4,"max_context_bytes":16384,"max_output_bytes":4096,"max_candidates":4,"max_runtime_ms":30000,"max_iterations":null,"max_output_tokens":null}}}));
        let mut host = self.read("host.json");
        host["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"peer.py"},{"path":format!("{id}.json")}],"grant_file":format!("{id}-grant.json"),"timeout_ms":15000}));
        self.write("host.json", host);
    }
    pub fn route(&self, id: &str, adapter: &str) {
        let mut runtime = self.read("runtime.json");
        runtime["routes"].as_array_mut().unwrap().push(json!({"id":id,"source":{"adapter_id":"example.input","capability_id":"task.submit"},"input_schema_id":"arcs.task.nearest.v1","payload_fields":["values","target"],"reasoner":{"adapter_id":adapter,"capability_id":"solution.propose"},"method":"learned","objective":"Find the nearest value","target_schema_id":"arcs.solution_candidate.nearest.v1","constraints":{},"budget":{"max_context_items":1,"max_context_bytes":8192,"max_output_bytes":4096,"max_candidates":1,"max_runtime_ms":10000,"max_iterations":null,"max_output_tokens":null}}));
        self.write("runtime.json", runtime);
    }
    pub fn open(&self) -> (Host, TaskRuntime) {
        let host = Host::open(self.directory.join("host.json")).unwrap();
        let runtime = TaskRuntime::open(
            &host,
            RuntimeConfig::load(self.directory.join("runtime.json")).unwrap(),
        )
        .unwrap();
        (host, runtime)
    }
    pub fn release(&self, id: &str) {
        std::fs::write(self.directory.join(format!("{id}.release")), "go").unwrap();
    }
    pub fn started(&self, id: &str) -> bool {
        self.directory.join(format!("{id}.started")).exists()
    }
    pub fn count(&self, id: &str, table: &str) -> i64 {
        rusqlite::Connection::open(self.directory.join(format!("{id}.sqlite")))
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
