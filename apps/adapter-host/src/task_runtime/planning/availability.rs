use super::PlanningConfig;
use crate::task_runtime::TaskRuntime;
use crate::{Host, HostError};
use arcs::core::VersionId;
use arcs::reasoning::{
    ContextSelection,
    plan_availability::{PlanAvailabilityOwner, ReasonerStatus, TemplateAvailability},
};

impl TaskRuntime {
    pub(super) fn template_availability(
        &self,
        host: &Host,
        config: &PlanningConfig,
        sequence: u64,
        input: &VersionId,
        revision: u8,
    ) -> Result<ContextSelection, HostError> {
        let owner = PlanAvailabilityOwner {
            definition: &self.journal.definition_version()?,
            input,
            sequence,
            revision,
        };
        if let Some(saved) = owner
            .load(host.store(), host.schemas())
            .map_err(HostError::Reasoning)?
        {
            return Ok(saved);
        }
        let mut templates = Vec::with_capacity(config.templates.len());
        for template in &config.templates {
            let mut reasoners = Vec::new();
            for choice in template.route.reasoner_choices() {
                let availability = host.reasoner_choice_availability(&choice, &[])?;
                reasoners.push(ReasonerStatus {
                    capability: choice.capability,
                    method: choice.method,
                    availability,
                });
            }
            templates.push(TemplateAvailability {
                id: &template.route.id,
                reasoners,
            });
        }
        owner
            .record(host.store(), host.schemas(), &templates)
            .map_err(HostError::Reasoning)
    }
}
