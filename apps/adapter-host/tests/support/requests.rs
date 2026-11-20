//! Independent Request peers with durable replies and explicit file gates.
#![allow(dead_code)]
use super::goals::Fixture;
use arcs_host::{Host, task_runtime::TaskRuntime};
use serde_json::json;
use std::time::{Duration, Instant};

const PEER: &str = r#"import json,pathlib,sqlite3,sys,time
root=pathlib.Path(sys.argv[1]).parent
c=json.load(open(sys.argv[1])); name=c['id']
db=sqlite3.connect(root/(name+'.sqlite'))
db.executescript('CREATE TABLE IF NOT EXISTS attempts(id TEXT); CREATE TABLE IF NOT EXISTS results(id TEXT PRIMARY KEY,input TEXT,response TEXT);')
for line in sys.stdin:
 p=json.loads(line); m=p['message']; kind=m['type']
 if kind=='hello':
  reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':name,'adapter_version':'1','capabilities':[{'id':'fan.read','contract':{'kind':'request','accepts':['arcs.request.fan.v1'],'emits':['arcs.state.fan.v1']},'required_permissions':['fan.read']}]}}
 elif kind=='request':
  inv=m['invocation']; key=inv['invocation_id']; raw=json.dumps(inv,sort_keys=True)
  with db: db.execute('INSERT INTO attempts VALUES(?)',(key,))
  old=db.execute('SELECT input,response FROM results WHERE id=?',(key,)).fetchone()
  if old:
   assert old[0]==raw
   reply=json.loads(old[1])
  else:
   payload={'device_id':'fan-01','enabled':False,'actuation_count':0}
   if c['mode']=='invalid': payload={'unexpected':'bad source payload'}
   reply={'type':'request_result','response':{'invocation_id':key,'external_reference':'fixture:'+key,'payload':payload}}
   with db: db.execute('INSERT INTO results VALUES(?,?,?)',(key,raw,json.dumps(reply)))
  (root/(name+'.started')).write_text(key)
  while c['mode'] in ('gate','failed_gate') and not (root/(name+'.release')).exists(): time.sleep(.005)
  if c['mode']=='failed_gate':
   reply={'type':'error','code':'offline','message':'simulated source failure','retryable':True}
 elif kind=='shutdown': reply={'type':'bye'}
 else: raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown': break
"#;

pub fn fixture() -> Fixture {
    let f = Fixture::new();
    f.manual();
    let mut config = f.read("runtime.json");
    config["max_concurrent_requests"] = json!(2);
    config["max_concurrent_reasoning"] = json!(2);
    config["max_jobs_per_tick"] = json!(10);
    // A pending Request must not consume an observation wait-tick budget.
    config["routes"][0]["goal"]["max_wait_ticks"] = json!(1);
    f.write("runtime.json", config);
    std::fs::write(f.directory.join("request-peer.py"), PEER).unwrap();
    f
}
pub fn use_default_limits(f: &Fixture) {
    let mut config = f.read("runtime.json");
    let options = config.as_object_mut().unwrap();
    options.remove("max_concurrent_reasoning");
    options.remove("max_concurrent_requests");
    f.write("runtime.json", config);
}
pub fn peer(f: &Fixture, id: &str, mode: &str) {
    f.write(&format!("{id}.json"), json!({"id":id,"mode":mode}));
    f.write(&format!("{id}-grant.json"),json!({"adapter_id":id,"producer_class":"adapter","enabled_capabilities":["fan.read"],"granted_permissions":["fan.read"],"assigned_trust":"medium","ingress_source_kind":"api","max_payload_bytes":4096,"max_external_reference_bytes":256,"reasoning_limits":null}));
    let mut host = f.read("host.json");
    host["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"request-peer.py"},{"path":format!("{id}.json")}],"grant_file":format!("{id}-grant.json"),"timeout_ms":15000}));
    f.write("host.json", host);
}
pub fn routes(f: &Fixture, peers: &[&str]) {
    let mut config = f.read("runtime.json");
    let template = config["routes"][0].clone();
    config["routes"] = json!(
        peers
            .iter()
            .map(|id| {
                let mut route = template.clone();
                route["id"] = json!(id);
                route["goal"]["sources"][0]["capability"]["adapter_id"] = json!(id);
                route
            })
            .collect::<Vec<_>>()
    );
    f.write("runtime.json", config);
}
pub fn started(f: &Fixture, id: &str) -> bool {
    f.directory.join(format!("{id}.started")).exists()
}
pub fn release(f: &Fixture, id: &str) {
    std::fs::write(f.directory.join(format!("{id}.release")), "go").unwrap();
}
pub fn count(f: &Fixture, id: &str, table: &str) -> i64 {
    f.db(id)
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
pub fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    mut ready: impl FnMut(&Host, &TaskRuntime) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready(host, runtime) {
        assert!(Instant::now() < deadline, "request qualification timed out");
        runtime.tick_at(host, 1_800_000_000_000).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
}
