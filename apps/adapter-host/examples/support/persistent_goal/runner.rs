use super::session::Session;
use super::*;

pub(super) fn run(options: &Options, evidence: &mut Evidence) -> Result<(), String> {
    let (_, query, delay) = validate(options)?;
    evidence.simulated_delay_ms = delay;
    let mut session = Session::open(options)?;
    let host_config: HostConfig = read(&options.host)?;
    evidence.core_database = Some(
        options
            .host
            .parent()
            .unwrap_or(Path::new("."))
            .join(host_config.database)
            .canonicalize()
            .map_err(|e| e.to_string())?,
    );
    check(
        session
            .runtime
            .goals_after(0, 1)
            .map_err(|e| e.to_string())?
            .is_empty(),
        "qualification requires fresh journals; existing work is preserved",
    )?;
    let initial = session.host.poll_inputs().map_err(|e| e.to_string())?;
    let note = initial
        .iter()
        .find(|a| a.schema_id.0 == "arcs.note.project_memory.v1")
        .ok_or("qualification note missing")?
        .version_id
        .clone();
    let first = session.until(
        options,
        options.start_unix_ms,
        GoalState::AwaitingApproval,
        evidence,
    )?;
    session.checkpoint("first_review", evidence)?;
    let before = session.observed(&first, false)?;
    check(
        before["enabled"] == false && before["actuation_count"] == 0,
        "fresh simulator must start off with zero effects",
    )?;
    let first_calls = session.attempts(&query)?;
    check(
        first_calls.len() == 2,
        "initial check and planning must have two completed attempts",
    )?;
    session.close()?;

    let mut session = Session::open(options)?;
    for _ in 0..3 {
        session
            .runtime
            .tick_at(&mut session.host, options.start_unix_ms)
            .map_err(|e| e.to_string())?;
    }
    let reopened = session.checkpoint("first_review_reopened", evidence)?;
    check(
        reopened.candidate == first.candidate
            && reopened.verification == first.verification
            && reopened.state == GoalState::AwaitingApproval
            && session.attempts(&query)? == first_calls,
        "restart changed a pending review or repeated reasoning",
    )?;
    session.approve(&reopened, true)?;
    let waiting = session.until(
        options,
        options.start_unix_ms,
        GoalState::WaitingTime,
        evidence,
    )?;
    session.checkpoint("waiting", evidence)?;
    let after_first = session.observed(&waiting, false)?;
    check(
        after_first["enabled"] == true && after_first["actuation_count"] == 1 && waiting.round == 2,
        "first confirmed effect must be a single switch on",
    )?;
    check(
        session.host.resource_usage().iter().all(|r| r.used == 0),
        "waiting goal retained compute resources",
    )?;
    let calls = session.attempts(&query)?;
    let due = options
        .start_unix_ms
        .checked_add(delay)
        .ok_or("simulated clock overflow")?;
    check(
        session
            .runtime
            .report(0)
            .map_err(|e| e.to_string())?
            .next_wake_unix_ms
            == Some(due),
        "wait deadline was not persisted",
    )?;
    session.close()?;

    let mut session = Session::open(options)?;
    for _ in 0..3 {
        session
            .runtime
            .tick_at(&mut session.host, due - 1)
            .map_err(|e| e.to_string())?;
    }
    let asleep = session.checkpoint("waiting_reopened_before_due", evidence)?;
    check(
        asleep.state == GoalState::WaitingTime
            && asleep.input_version_id == first.input_version_id
            && session.attempts(&query)? == calls,
        "sleeping goal woke early or repeated inference",
    )?;
    session.close()?;

    let mut session = Session::open(options)?;
    let second = session.until(options, due, GoalState::AwaitingApproval, evidence)?;
    session.checkpoint("second_review", evidence)?;
    check(
        second.input_version_id == first.input_version_id
            && second.candidate != first.candidate
            && second.before.iter().all(|id| !first.before.contains(id)),
        "waking did not acquire fresh context and a new candidate",
    )?;
    check(
        session
            .runtime
            .review_goal(
                &session.host,
                second.sequence,
                first.candidate.as_ref().ok_or("missing first candidate")?,
                ApprovalDecision::Approved,
                Actor {
                    actor_type: ActorType::Human,
                    id: "qualification.simulator-operator".into(),
                },
                "Reject stale candidate".into(),
            )
            .is_err(),
        "old candidate could authorize a later round",
    )?;
    session.approve(&second, false)?;
    let complete = session.until(options, due, GoalState::Completed, evidence)?;
    session.checkpoint("completed", evidence)?;
    let final_state = session.observed(&complete, true)?;
    check(
        final_state["enabled"] == false && final_state["actuation_count"] == 2,
        "final observation does not prove exactly two state changes",
    )?;
    let outcome = session.artifact(complete.outcome.as_ref().ok_or("missing outcome")?)?;
    check(
        outcome.schema_id.0 == "arcs.outcome.v1" && outcome.payload["verdict"] == "success",
        "missing confirmed outcome",
    )?;
    let page = session
        .host
        .reasoning_attempts(&query)
        .map_err(|e| e.to_string())?;
    check(
        page.attempts.len() == 6 && page.page_counts.solved_candidate_responses == 6,
        "expected six valid check/plan/assessment attempts",
    )?;
    for entry in page.attempts {
        let request = session.artifact(&entry.attempt.request_version_id)?;
        let inv: ReasoningInvocationV2 =
            serde_json::from_value(request.payload["invocation"].clone())
                .map_err(|e| e.to_string())?;
        check(
            inv.context
                .iter()
                .any(|c| c.version_id == first.input_version_id),
            "original task missing from persisted context",
        )?;
        let memory = inv
            .context
            .iter()
            .find(|c| c.version_id == note)
            .ok_or("stored note missing from a model request")?;
        check(
            memory
                .payload
                .as_object()
                .is_some_and(|p| p.len() == 1 && p.contains_key("body")),
            "private memory fields entered model context",
        )?;
        if let AttemptOutcome::Response {
            status: ReasoningStatus::Solved,
            result_version_id,
            ..
        } = entry.attempt.outcome
        {
            let result = session.artifact(&result_version_id)?;
            evidence.adapter_reported_models.push(json!({"request_version_id":request.version_id,"implementation":result.payload["response"]["trace"]["implementation"],"model":result.payload["response"]["trace"]["details"]["model"]}));
        } else {
            return Err("non-solved reasoning outcome".into());
        }
        evidence.attempt_versions.push(entry.version_id);
    }
    session.close()?;
    let mut session = Session::open(options)?;
    for _ in 0..3 {
        session
            .runtime
            .tick_at(&mut session.host, due)
            .map_err(|e| e.to_string())?;
    }
    let replay = session.checkpoint("completed_reopened", evidence)?;
    check(
        replay.state == GoalState::Completed
            && replay.execution == complete.execution
            && replay.outcome == complete.outcome
            && session.attempts(&query)? == evidence.attempt_versions,
        "completed replay changed effects or repeated reasoning",
    )?;
    session.close()
}
