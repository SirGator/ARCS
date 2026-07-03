#include "verification/verifier.hpp"

#include "reducer/time_source.hpp"
#include "store/store.hpp"

#include <string>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

std::optional<ArtifactRef> json_ref(const nlohmann::json& payload, const char* key)
{
    if (!payload.is_object() || !payload.contains(key) || !payload.at(key).is_object()) {
        return std::nullopt;
    }

    const auto& value = payload.at(key);
    if (!value.contains("artifact_id") || !value.at("artifact_id").is_string() ||
        !value.contains("version_id") || !value.at("version_id").is_string()) {
        return std::nullopt;
    }

    return ArtifactRef{
        .artifact_id = value.at("artifact_id").get<std::string>(),
        .version_id = value.at("version_id").get<std::string>(),
    };
}

bool approval_not_expired(const ArtifactVersion& approval, const VerificationContext& context)
{
    if (context.time_source == nullptr) {
        return true;
    }

    const auto expires_at = approval.payload.value("expires_at", std::string{});
    return expires_at.empty() || context.time_source->now() <= expires_at;
}

} // namespace

VerificationCheck ApprovalVerifier::check(
    const ArtifactVersion& target,
    const VerificationContext& context) const {
    VerificationCheck result{};
    result.name = "approval";

    if (target.type != "action") {
        result.status = CheckStatus::Pass;
        result.detail = "approval binding not required for target type";
        return result;
    }

    if (context.policy == nullptr || context.store == nullptr) {
        result.status = CheckStatus::Unknown;
        result.detail = "approval verification context incomplete";
        return result;
    }

    const auto option_ref = json_ref(target.payload, "option_ref");
    const auto policy_ref = json_ref(target.payload, "policy_ref");
    if (!option_ref.has_value() || !policy_ref.has_value()) {
        result.status = CheckStatus::Unknown;
        result.detail = "action binding refs missing";
        return result;
    }

    const auto approvals = context.store->list(arcs::store::ListQuery{.type = std::string{"approval"}});
    for (const auto& approval : approvals) {
        if (!approval.payload.is_object()) {
            continue;
        }

        if (approval.payload.value("decision", std::string{}) != "approve") {
            continue;
        }

        const auto approval_action_ref = json_ref(approval.payload, "action_ref");
        const auto approval_option_ref = json_ref(approval.payload, "target_option");
        const auto approval_policy_ref = json_ref(approval.payload, "policy_ref");
        if (!approval_action_ref.has_value() || !approval_option_ref.has_value() || !approval_policy_ref.has_value()) {
            continue;
        }

        if (approval_action_ref->artifact_id != target.artifact_id ||
            approval_action_ref->version_id != target.version_id) {
            continue;
        }

        if (*approval_option_ref != *option_ref) {
            continue;
        }

        if (*approval_policy_ref != *policy_ref) {
            continue;
        }

        if (approval.payload.value("approval_scope", std::string{}) != target.stream_key) {
            result.status = CheckStatus::Fail;
            result.detail = "approval scope does not match action stream";
            return result;
        }

        if (!approval_not_expired(approval, context)) {
            result.status = CheckStatus::Fail;
            result.detail = "approval expired";
            return result;
        }

        if (policy_ref->artifact_id != context.policy->artifact_id ||
            policy_ref->version_id != context.policy->version_id) {
            result.status = CheckStatus::Fail;
            result.detail = "approval bound to stale policy head";
            return result;
        }

        result.status = CheckStatus::Pass;
        result.detail = "matching approval bound to action";
        return result;
    }

    result.status = CheckStatus::Fail;
    result.detail = "missing approval bound to action";
    return result;
}

} // namespace arcs::verification
