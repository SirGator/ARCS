/**
 * @file approval_verifier.cpp
 * @brief Implements ApprovalVerifier::check, which confirms that an
 *        "action" target is backed by a matching, unexpired approval
 *        artifact bound to the same policy head, action candidate, option,
 *        and store head.
 */
#include "verification/verifier.hpp"

#include "reducer/time_source.hpp"
#include "store/store.hpp"

#include <string>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

/**
 * @brief Extracts an ArtifactRef from a named object field of a JSON
 *        payload, if present and well-formed.
 * @param payload JSON object to read the field from.
 * @param key Name of the field expected to hold an artifact/version id pair.
 * @return The parsed ArtifactRef, or std::nullopt if the field is missing
 *         or malformed.
 */
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

/**
 * @brief Determines whether an approval artifact has not yet expired.
 * @param approval Approval artifact to check.
 * @param context Verification context; if no time_source is set, approvals
 *                are treated as never expired.
 * @return True if the approval has no expiry or has not yet expired.
 */
bool approval_not_expired(const ArtifactVersion& approval, const VerificationContext& context)
{
    if (context.time_source == nullptr) {
        return true;
    }

    const auto expires_at = approval.payload.value("expires_at", std::string{});
    return expires_at.empty() || context.time_source->now() <= expires_at;
}

/**
 * @brief Compares two optional ArtifactRef values for equality, treating
 *        an absent value on either side as a mismatch.
 * @param left First optional reference.
 * @param right Second optional reference.
 * @return True if both are present and equal.
 */
bool ref_matches(const std::optional<ArtifactRef>& left, const std::optional<ArtifactRef>& right)
{
    return left.has_value() && right.has_value() && *left == *right;
}

} // namespace

/**
 * @brief Verifies that an "action" target has a matching approval on
 *        record. Non-action targets pass trivially. Checks that the
 *        approval's action-candidate, option, and policy refs match the
 *        target, that its verification_ref and scope agree, that it has
 *        not expired, and that it is still bound to the current policy
 *        and store heads.
 * @param target Artifact version being verified.
 * @param context Verification context; must provide a policy and store.
 * @return VerificationCheck named "approval" with the outcome.
 */
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
    const auto action_candidate_ref = json_ref(target.payload, "action_candidate_ref");
    const auto approval_ref = json_ref(target.payload, "approval_ref");
    const auto verification_ref = json_ref(target.payload, "verification_ref");
    if (!option_ref.has_value() || !policy_ref.has_value() || !action_candidate_ref.has_value() ||
        !approval_ref.has_value() || !verification_ref.has_value()) {
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

        const auto approval_action_candidate_ref = json_ref(approval.payload, "action_candidate_ref");
        const auto approval_option_ref = json_ref(approval.payload, "target_option");
        const auto approval_policy_ref = json_ref(approval.payload, "policy_ref");
        const auto approval_verification_ref = json_ref(approval.payload, "verification_ref");
        if (!approval_action_candidate_ref.has_value() || !approval_option_ref.has_value() ||
            !approval_policy_ref.has_value() || !approval_verification_ref.has_value()) {
            continue;
        }

        if (approval.artifact_id != approval_ref->artifact_id ||
            approval.version_id != approval_ref->version_id) {
            continue;
        }

        if (*approval_action_candidate_ref != *action_candidate_ref) {
            continue;
        }

        if (*approval_option_ref != *option_ref) {
            continue;
        }

        if (*approval_policy_ref != *policy_ref) {
            continue;
        }

        if (!ref_matches(approval_verification_ref, verification_ref)) {
            result.status = CheckStatus::Fail;
            result.detail = "approval verification_ref does not match action";
            return result;
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

        const auto store_head_at_approval = approval.payload.value("store_head_at_approval", std::string{});
        const auto current_action_candidate_head =
            context.store->current_head_version_id(action_candidate_ref->artifact_id);
        if (!store_head_at_approval.empty() && current_action_candidate_head.has_value() &&
            store_head_at_approval != *current_action_candidate_head) {
            result.status = CheckStatus::Fail;
            result.detail = "approval store head no longer matches action candidate head";
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
