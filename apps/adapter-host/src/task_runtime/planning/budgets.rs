//! Conservative bounds include every configured goal round and assessment.
use super::PlanningConfig;
use crate::HostError;
use crate::task_runtime::{ReasoningRoute, config::invalid};
use arcs::reasoning::{ReasoningMethod, ReasoningResources};

#[derive(Default)]
pub(super) struct PlanBudget {
    runtime: u64,
    tokens: usize,
    actions: usize,
}
impl PlanBudget {
    pub(super) fn add(
        &mut self,
        route: &ReasoningRoute,
        bounded_tokens: bool,
    ) -> Result<(), HostError> {
        let rounds = route.goal.as_ref().map_or(1, |g| g.max_rounds as usize);
        let mut maximum_runtime = 0;
        let mut maximum_tokens = 0;
        for choice in route.reasoner_choices() {
            let mut option = Self::default();
            option.reasoning(&choice.budget, choice.method, rounds, bounded_tokens)?;
            maximum_runtime = maximum_runtime.max(option.runtime);
            maximum_tokens = maximum_tokens.max(option.tokens);
        }
        if let Some(policy) = &route.reasoning_recovery {
            for choice in &policy.alternatives {
                let mut option = Self::default();
                option.reasoning(&choice.budget, choice.method, rounds, bounded_tokens)?;
                maximum_runtime = maximum_runtime
                    .checked_add(option.runtime)
                    .ok_or_else(|| invalid("plan recovery runtime budget overflow"))?;
                maximum_tokens = maximum_tokens
                    .checked_add(option.tokens)
                    .ok_or_else(|| invalid("plan recovery token budget overflow"))?;
            }
        }
        self.runtime = self
            .runtime
            .checked_add(maximum_runtime)
            .ok_or_else(|| invalid("plan runtime budget overflow"))?;
        self.tokens = self
            .tokens
            .checked_add(maximum_tokens)
            .ok_or_else(|| invalid("plan token budget overflow"))?;
        if let Some(goal) = &route.goal {
            self.actions = self
                .actions
                .checked_add(rounds)
                .ok_or_else(|| invalid("plan action budget overflow"))?;
            let assessments = rounds * (1 + usize::from(goal.initial_assessment));
            self.reasoning(
                &goal.assessment.budget,
                goal.assessment.method,
                assessments,
                bounded_tokens,
            )?;
        }
        Ok(())
    }
    fn reasoning(
        &mut self,
        budget: &ReasoningResources,
        method: ReasoningMethod,
        calls: usize,
        bounded_tokens: bool,
    ) -> Result<(), HostError> {
        self.runtime = budget
            .max_runtime_ms
            .checked_mul(calls as u64)
            .and_then(|n| self.runtime.checked_add(n))
            .ok_or_else(|| invalid("plan runtime budget overflow"))?;
        if bounded_tokens && method.uses_model() {
            let tokens = budget
                .max_output_tokens
                .ok_or_else(|| invalid("plan token budget requires bounded model output tokens"))?;
            self.tokens = tokens
                .checked_mul(calls)
                .and_then(|n| self.tokens.checked_add(n))
                .ok_or_else(|| invalid("plan token budget overflow"))?;
        }
        Ok(())
    }
    pub(super) fn check(&self, config: &PlanningConfig) -> Result<(), HostError> {
        if self.runtime > config.max_total_runtime_ms
            || config
                .max_total_output_tokens
                .is_some_and(|n| self.tokens > n)
            || config.max_total_actions.is_some_and(|n| self.actions > n)
        {
            return Err(invalid(
                "plan aggregate reasoning or action budget exceeded",
            ));
        }
        Ok(())
    }
}
