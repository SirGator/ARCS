/**
 * @file action_service.cpp
 * @brief Implements ActionService, the pipeline stage responsible for turning an
 *        approved decision option into a concrete action candidate: materializing
 *        candidate actions from an option/policy pair, summarizing their risk,
 *        promoting a candidate into an executable action once approved, and
 *        assembling the verification context used to check permissions and policy
 *        compliance before execution.
 */

#include "core/services/action_service.hpp"

#include "core/services/approval_service.hpp"
#include "materializer.hpp"

namespace arcs::core::services {

/**
 * @brief Materializes the first action candidate derived from an option under a given policy.
 * @param option The artifact version representing the decision option to materialize.
 * @param policy The policy artifact version used to constrain materialization.
 * @return The first materialized action candidate, or std::nullopt if none were produced.
 */
std::optional<arcs::artifact::ArtifactVersion> ActionService::materialize_candidate(
    const arcs::artifact::ArtifactVersion& option,
    const arcs::artifact::ArtifactVersion& policy) const
{
    arcs::execution::ActionMaterializer materializer;
    auto actions = materializer.materialize(option, policy);
    if (actions.empty()) {
        return std::nullopt;
    }
    return actions.front();
}

/**
 * @brief Builds a short human-readable risk summary string combining the option's
 *        safety level and the action candidate's type.
 * @param option The artifact version whose "safety_level" payload field is read.
 * @param action_candidate The action candidate artifact whose "type" payload field is read.
 * @return A string of the form "safety_level=<level>; action_type=<type>".
 */
std::string ActionService::risk_summary(
    const arcs::artifact::ArtifactVersion& option,
    const arcs::artifact::ArtifactVersion& action_candidate) const
{
    const auto safety_level = option.payload.value("safety_level", std::string{"unknown"});
    const auto action_type = action_candidate.payload.value("type", std::string{"unknown"});
    return "safety_level=" + safety_level + "; action_type=" + action_type;
}

/**
 * @brief Promotes an action candidate to an executable action by delegating to
 *        ApprovalService once approval has been granted.
 * @param action_candidate The action candidate artifact to promote.
 * @param approval The approval artifact authorizing the promotion.
 * @param approval_service The service used to perform the actual promotion.
 * @return The promoted, executable action artifact version.
 */
arcs::artifact::ArtifactVersion ActionService::promote_candidate(
    const arcs::artifact::ArtifactVersion& action_candidate,
    const arcs::approval::ApprovalArtifact& approval,
    const ApprovalService& approval_service) const
{
    return approval_service.promote_action_candidate(action_candidate, approval);
}

/**
 * @brief Assembles a VerificationContext bundling references to the policy,
 *        effective permissions, schema registry, store, and time source needed
 *        to verify an action candidate.
 * @param policy The policy artifact version to attach to the context.
 * @param permissions The effective permissions applicable to the verification.
 * @param schemas The schema registry used for payload validation.
 * @param store The artifact store used to resolve referenced artifacts.
 * @param time_source The time source used for time-dependent verification checks.
 * @return A populated VerificationContext referencing the provided inputs.
 */
arcs::verification::VerificationContext ActionService::build_verification_context(
    const arcs::artifact::ArtifactVersion& policy,
    const arcs::reducer::EffectivePermissions& permissions,
    const arcs::schema::SchemaRegistry& schemas,
    const arcs::store::IStore& store,
    const arcs::reducer::ITimeSource& time_source) const
{
    arcs::verification::VerificationContext context{};
    context.policy = &policy;
    context.permissions = permissions;
    context.schema_registry = &schemas;
    context.store = &store;
    context.time_source = &time_source;
    return context;
}

} // namespace arcs::core::services
