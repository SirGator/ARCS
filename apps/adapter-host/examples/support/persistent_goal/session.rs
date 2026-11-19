use super::*;

pub(super) struct Session {
    pub(super) runtime: TaskRuntime,
    pub(super) host: Host,
}
impl Session {
    pub(super) fn open(options: &Options) -> Result<Self, String> {
        let host = Host::open(&options.host).map_err(|e| e.to_string())?;
        let runtime = TaskRuntime::open(
            &host,
            RuntimeConfig::load(&options.runtime).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { runtime, host })
    }
    pub(super) fn close(mut self) -> Result<(), String> {
        self.host.shutdown().map_err(|e| e.to_string())
    }
    pub(super) fn goal(&self) -> Result<GoalTask, String> {
        let mut goals = self.runtime.goals_after(0, 2).map_err(|e| e.to_string())?;
        check(goals.len() == 1, "expected exactly one persisted goal")?;
        Ok(goals.remove(0))
    }
    pub(super) fn until(
        &mut self,
        options: &Options,
        at: u64,
        state: GoalState,
        evidence: &mut Evidence,
    ) -> Result<GoalTask, String> {
        let deadline = Instant::now() + options.phase_timeout;
        loop {
            self.runtime
                .tick_at(&mut self.host, at)
                .map_err(|e| e.to_string())?;
            if let Some(goal) = self
                .runtime
                .goals_after(0, 2)
                .map_err(|e| e.to_string())?
                .into_iter()
                .next()
            {
                if goal.state == state {
                    return Ok(goal);
                }
                if goal.state.terminal() {
                    self.checkpoint("unexpected_terminal", evidence)?;
                    return Err(format!(
                        "goal stopped before {state:?}: {:?}: {}",
                        goal.state, goal.detail
                    ));
                }
            }
            check(
                Instant::now() < deadline,
                "qualification phase exceeded wall-clock limit",
            )?;
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    pub(super) fn attempts(&self, query: &AttemptQuery) -> Result<Vec<VersionId>, String> {
        let page = self
            .host
            .reasoning_attempts(query)
            .map_err(|e| e.to_string())?;
        check(
            !page.has_more
                && page.skipped_untrusted == 0
                && page.skipped_oversized == 0
                && page.page_counts.failures == 0,
            "incomplete or failed reasoning evidence",
        )?;
        Ok(page.attempts.into_iter().map(|a| a.version_id).collect())
    }
    pub(super) fn approve(
        &mut self,
        goal: &GoalTask,
        expected_enabled: bool,
    ) -> Result<(), String> {
        let candidate = self.artifact(goal.candidate.as_ref().ok_or("missing candidate")?)?;
        let before = self.observed(goal, false)?;
        check(
            candidate.payload["enabled"] == expected_enabled
                && candidate.payload["device_id"] == before["device_id"],
            "model proposed a different step than the qualification scenario permits",
        )?;
        self.runtime
            .review_goal(
                &self.host,
                goal.sequence,
                goal.candidate.as_ref().ok_or("missing candidate")?,
                ApprovalDecision::Approved,
                Actor {
                    actor_type: ActorType::Human,
                    id: "qualification.simulator-operator".into(),
                },
                "Explicit test review of the known simulated actuator only".into(),
            )
            .map_err(|e| e.to_string())
    }
    pub(super) fn artifact(&self, version: &VersionId) -> Result<Artifact, String> {
        self.host
            .store()
            .get(version)
            .map_err(|e| format!("{e:?}"))?
            .ok_or_else(|| "missing proof artifact".into())
    }
    pub(super) fn observed(&self, goal: &GoalTask, after: bool) -> Result<Value, String> {
        for id in if after { &goal.after } else { &goal.before } {
            let a = self.artifact(id)?;
            if a.schema_id.0 == "arcs.state.fan.v1" {
                return Ok(a.payload);
            }
        }
        Err("missing observed simulator state".into())
    }
    pub(super) fn checkpoint(
        &self,
        name: &str,
        evidence: &mut Evidence,
    ) -> Result<GoalTask, String> {
        let goal = self.goal()?;
        evidence
            .checkpoints
            .push(json!({"stage":name,"goal":goal,"resources":self.host.resource_usage()}));
        eprintln!(
            "qualification checkpoint: {name}: {:?}, round {}",
            goal.state, goal.round
        );
        Ok(goal)
    }
}
