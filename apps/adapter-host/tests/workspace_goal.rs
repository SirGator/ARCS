#[path = "workspace_goal/repair.rs"]
mod repair;
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType, VersionId};
use arcs::store::{ArtifactRelations, relation_kinds};
use arcs_host::Host;
use arcs_host::task_runtime::{
    RuntimeConfig, TaskRuntime,
    goal::{GoalState, GoalTask},
};
use serde_json::{Value, json};

const NOW: u64 = 1_800_000_000_000;
const MONTH: u64 = 30 * 24 * 60 * 60 * 1000;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let fixture = Self(std::env::temp_dir().join(format!(
            "arcs-workspace-goal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/workspace-goal");
        fs::create_dir_all(fixture.0.join("project")).unwrap();
        for name in [
            "input-profile.json",
            "input-grant.json",
            "reasoner-profile.json",
            "reasoner-grant.json",
            "check-profile.json",
            "check-grant.json",
            "task.schema.json",
            "action-policy.schema.json",
            "success-policy.schema.json",
            "project/calculator.py",
            "project/test_calculator.py",
        ] {
            fs::copy(source.join(name), fixture.0.join(name)).unwrap();
        }
        let mut host: Value =
            serde_json::from_slice(&fs::read(source.join("host.json")).unwrap()).unwrap();
        host["database"] = json!("core.sqlite");
        for schema in host["schemas"].as_array_mut().unwrap() {
            let path = schema.as_str().unwrap();
            if path.starts_with("../") {
                *schema = json!(source.join(path).canonicalize().unwrap());
            }
        }
        for adapter in host["adapters"].as_array_mut().unwrap() {
            let program = Path::new(adapter["program"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_str()
                .unwrap();
            adapter["program"] = json!(support::adapter_binary(program));
            let journal = Path::new(adapter["args"][0]["path"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_owned();
            adapter["args"][0]["path"] = json!(journal.to_str().unwrap());
        }
        fixture.write("host.json", host);
        let mut runtime: Value =
            serde_json::from_slice(&fs::read(source.join("runtime.json")).unwrap()).unwrap();
        runtime["journal"] = json!("runtime.sqlite");
        runtime["routes"][0]["goal"]["between_rounds"]["delay_ms"] = json!(MONTH);
        fixture.write("runtime.json", runtime);
        fixture
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn write(&self, name: &str, value: Value) {
        fs::write(self.path(name), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.path(name)).unwrap()).unwrap()
    }
    fn open(&self) -> (Host, TaskRuntime) {
        let host = Host::open(self.path("host.json")).unwrap();
        let runtime = TaskRuntime::open(
            &host,
            RuntimeConfig::load(self.path("runtime.json")).unwrap(),
        )
        .unwrap();
        (host, runtime)
    }
    fn checks(&self) -> u32 {
        rusqlite::Connection::open(self.path("check.sqlite"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM checks WHERE result IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    now: u64,
    ready: impl Fn(GoalState) -> bool,
) -> GoalTask {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        runtime.tick_at(host, now).unwrap();
        if let Some(task) = runtime.goals_after(0, 1).unwrap().pop() {
            if ready(task.state) {
                return task;
            }
            assert!(!task.state.terminal(), "{task:?}");
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            runtime.report(0).unwrap()
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}

fn approval(
    host: &Host,
    runtime: &mut TaskRuntime,
    goal: &GoalTask,
    candidate: &VersionId,
) -> Result<(), arcs_host::HostError> {
    runtime
        .review_goal(
            host,
            goal.sequence,
            candidate,
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::Human,
                id: "test.maintainer".into(),
            },
            "Review the configured workspace action".into(),
        )
        .map(|_| ())
}

fn result(host: &Host, version: Option<&VersionId>) -> Value {
    host.store().get(version.unwrap()).unwrap().unwrap().payload
}

#[test]
fn a_project_check_fails_then_passes_after_a_month_and_a_fresh_approval() {
    let f = Fixture::new();
    fs::write(
        f.path("project/calculator.py"),
        "def add(left, right):\n    return left - right\n",
    )
    .unwrap();
    let (mut host, mut runtime) = f.open();
    let first = until(&mut host, &mut runtime, NOW, |state| {
        state == GoalState::AwaitingApproval
    });
    assert_eq!(f.checks(), 0);
    approval(
        &host,
        &mut runtime,
        &first,
        first.candidate.as_ref().unwrap(),
    )
    .unwrap();
    let waiting = until(&mut host, &mut runtime, NOW, |state| {
        state == GoalState::WaitingTime
    });
    assert_eq!(waiting.round, 2);
    assert_eq!(
        result(&host, waiting.execution.as_ref())["result"]["status"],
        "failed"
    );
    assert_eq!(
        result(&host, waiting.outcome.as_ref())["verdict"],
        "failure"
    );
    assert_eq!(f.checks(), 1);
    let first_receipt = waiting.execution.unwrap();
    host.shutdown().unwrap();
    drop((host, runtime));

    fs::write(
        f.path("project/calculator.py"),
        "def add(left, right):\n    return left + right\n",
    )
    .unwrap();
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, NOW + MONTH - 1).unwrap();
    assert_eq!(
        runtime.goals_after(0, 1).unwrap()[0].state,
        GoalState::WaitingTime
    );
    assert_eq!(f.checks(), 1);
    let second = until(&mut host, &mut runtime, NOW + MONTH, |state| {
        state == GoalState::AwaitingApproval
    });
    assert_eq!(second.sequence, first.sequence);
    assert_eq!(second.input_version_id, first.input_version_id);
    assert_ne!(second.candidate, first.candidate);
    assert!(
        approval(
            &host,
            &mut runtime,
            &second,
            first.candidate.as_ref().unwrap()
        )
        .is_err()
    );
    assert_eq!(f.checks(), 1);
    approval(
        &host,
        &mut runtime,
        &second,
        second.candidate.as_ref().unwrap(),
    )
    .unwrap();
    let done = until(&mut host, &mut runtime, NOW + MONTH, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(f.checks(), 2);
    let execution = done.execution.as_ref().unwrap();
    assert_ne!(execution, &first_receipt);
    assert_eq!(result(&host, Some(execution))["result"]["status"], "passed");
    assert_eq!(
        result(&host, Some(&first_receipt))["result"]["status"],
        "failed"
    );
    let evidence = ArtifactRelations::new(host.store())
        .outgoing(done.outcome.as_ref().unwrap())
        .unwrap();
    assert!(
        evidence
            .iter()
            .any(|edge| &edge.to == execution && edge.kind == relation_kinds::supported_by())
    );
    let assessment = result(&host, done.assessment.as_ref());
    assert_eq!(assessment["evidence_version_ids"], json!([execution]));
    assert!(done.after.is_empty());
    host.shutdown().unwrap();
}

#[test]
fn execution_success_requires_a_matching_policy_and_a_reference_to_the_current_receipt() {
    for missing_reference in [false, true] {
        let f = Fixture::new();
        if missing_reference {
            let assessor = include_str!("support/planner_peer.py").replace(
                "   status='no_solution'",
                "   payload['evidence_version_ids']=[inv['constraints']['goal_runtime']['task_version_id']]\n   status='no_solution'",
            );
            fs::write(f.path("assessor.py"), assessor).unwrap();
            f.write("assessor.json",json!({
                "adapter_id":"workspace.unproven","target_schema_id":"arcs.goal_assessment_candidate.v1",
                "context_namespace":"goal_runtime","iterations":1,"proposal":{
                    "goal_status":"achieved","effect":"success","detail":"asserted success without its receipt","evidence_version_ids":[]
                }
            }));
            let mut grant = f.read("reasoner-grant.json");
            grant["adapter_id"] = json!("workspace.unproven");
            f.write("assessor-grant.json", grant);
            let mut host = f.read("host.json");
            host["adapters"].as_array_mut().unwrap().push(json!({
                "program":"/usr/bin/python3","args":[{"path":"assessor.py"},{"path":"assessor.json"}],
                "grant_file":"assessor-grant.json","timeout_ms":10000
            }));
            f.write("host.json", host);
            let mut config = f.read("runtime.json");
            config["routes"][0]["goal"]["assessment"]["capability"]["adapter_id"] =
                json!("workspace.unproven");
            f.write("runtime.json", config);
        } else {
            let mut policy = f.read("success-policy.schema.json");
            policy["properties"]["result"]["properties"]["check"]["const"] = json!("integration");
            f.write("success-policy.schema.json", policy);
        }
        let (mut host, mut runtime) = f.open();
        let request = until(&mut host, &mut runtime, NOW, |state| {
            state == GoalState::AwaitingApproval
        });
        approval(
            &host,
            &mut runtime,
            &request,
            request.candidate.as_ref().unwrap(),
        )
        .unwrap();
        let done = until(&mut host, &mut runtime, NOW, GoalState::terminal);
        assert_eq!(f.checks(), 1);
        assert_eq!(
            done.state,
            GoalState::Failed,
            "missing reference={missing_reference}: {done:?}"
        );
        assert!(done.outcome.is_none());
        assert!(
            done.detail.contains(if missing_reference {
                "reference the current execution receipt"
            } else {
                "does not meet success schema"
            }),
            "{done:?}"
        );
        assert_eq!(
            result(&host, done.execution.as_ref())["result"]["status"],
            "passed"
        );
        host.shutdown().unwrap();
    }
}

#[test]
fn a_goal_without_sources_requires_an_explicit_post_execution_success_policy() {
    let f = Fixture::new();
    let host = Host::open(f.path("host.json")).unwrap();
    let configured = f.read("runtime.json");
    for initial_assessment in [false, true] {
        let mut config = configured.clone();
        if initial_assessment {
            config["routes"][0]["goal"]["initial_assessment"] = json!(true);
        } else {
            config["routes"][0]["goal"]
                .as_object_mut()
                .unwrap()
                .remove("execution_success_schema_id");
        }
        f.write("runtime.json", config);
        let result = TaskRuntime::open(&host, RuntimeConfig::load(f.path("runtime.json")).unwrap());
        assert!(
            matches!(result,Err(error) if error.to_string().contains("goals without sources require"))
        );
    }
    assert_eq!(f.checks(), 0);
}
