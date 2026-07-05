/**
 * @file resume_service.cpp
 * @brief Implements ResumeService, which reconstructs the state needed to
 * resume execution after an approval decision by re-fetching the artifacts
 * referenced from the approval's payload.
 */
#include "core/resume/resume_service.hpp"

#include <chrono>
#include <optional>
#include <ctime>
#include <iomanip>
#include <sstream>

namespace arcs::core::resume {
namespace {

std::string utc_now()
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());
    std::tm tm{};

#if defined(_WIN32)
    gmtime_s(&tm, &now_time_t);
#else
    gmtime_r(&now_time_t, &tm);
#endif

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
    return out.str();
}

/**
 * @brief Looks up an artifact referenced by an "artifact_id" field nested
 * under the given key in a JSON payload, if that key names an object and
 * the referenced artifact exists in the store.
 * @param store Store to resolve the artifact from.
 * @param payload JSON payload potentially containing the reference.
 * @param key Name of the nested object field holding the reference.
 * @return The resolved artifact, or std::nullopt if the reference is
 *         missing, malformed, or not found in the store.
 */
std::optional<arcs::verification::ArtifactRef> json_ref(
    const nlohmann::json& payload,
    const char* key)
{
    if (!payload.is_object() || !payload.contains(key) || !payload.at(key).is_object()) {
        return std::nullopt;
    }

    const auto& value = payload.at(key);
    const auto artifact_id = value.value("artifact_id", std::string{});
    const auto version_id = value.value("version_id", std::string{});
    if (artifact_id.empty() || version_id.empty()) {
        return std::nullopt;
    }

    return arcs::verification::ArtifactRef{.artifact_id = artifact_id, .version_id = version_id};
}

std::optional<arcs::artifact::ArtifactVersion> load_ref(
    const arcs::store::IStore& store,
    const nlohmann::json& payload,
    const char* key)
{
    const auto ref = json_ref(payload, key);
    if (!ref.has_value()) {
        return std::nullopt;
    }

    if (!store.has_version(ref->version_id)) {
        return std::nullopt;
    }

    const auto artifact = store.get_version(ref->version_id);
    if (artifact.artifact_id != ref->artifact_id) {
        return std::nullopt;
    }

    return artifact;
}

bool ref_matches(
    const std::optional<arcs::verification::ArtifactRef>& left,
    const std::optional<arcs::verification::ArtifactRef>& right)
{
    return left.has_value() && right.has_value() && *left == *right;
}

bool decision_is_approve(const nlohmann::json& payload)
{
    return payload.is_object() && payload.value("decision", std::string{}) == "approve";
}

bool approval_not_expired(const nlohmann::json& payload)
{
    const auto expires_at = payload.value("expires_at", std::string{});
    return expires_at.empty() || utc_now() <= expires_at;
}

ApprovalResumeState fail_state(const std::string& error_code, const std::string& error)
{
    ApprovalResumeState state{};
    state.ok = false;
    state.error_code = error_code;
    state.error = error;
    return state;
}

} // namespace

/**
 * @brief Rebuilds the resume state for an approval by re-resolving the
 * option, policy, and action candidate artifacts referenced in its
 * payload. On success, it reloads the persisted verification report and
 * verifies that approval, request, candidate, option, policy, scope, risk,
 * and expiry still match exactly.
 * @param approval The approval artifact to resume from.
 * @param store Store used to resolve referenced artifacts.
 * @return An ApprovalResumeState with ok=false and an error message if any
 *         referenced artifact could not be resolved, otherwise ok=true
 *         with the resolved artifacts populated.
 */
ApprovalResumeState ResumeService::resume_from_approval(
    const arcs::approval::ApprovalArtifact& approval,
    const arcs::store::IStore& store) const
{
    const auto option = load_ref(store, approval.payload, "target_option");
    const auto policy = load_ref(store, approval.payload, "policy_ref");
    const auto candidate = load_ref(store, approval.payload, "action_candidate_ref");
    const auto request = load_ref(store, approval.payload, "request_ref");
    const auto verification_report = load_ref(store, approval.payload, "verification_ref");
    if (!option.has_value() || !policy.has_value() || !candidate.has_value() ||
        !request.has_value() || !verification_report.has_value()) {
        return fail_state("resume.artifacts_missing", "resume artifacts missing in store");
    }

    if (!decision_is_approve(approval.payload)) {
        return fail_state("resume.approval_not_approved", "approval decision is not approve");
    }

    if (!approval_not_expired(approval.payload)) {
        return fail_state("resume.approval_expired", "approval expired");
    }

    if (!verification_report->payload.is_object()) {
        return fail_state("resume.verification_report_invalid", "verification report invalid");
    }

    const auto option_ref = json_ref(approval.payload, "target_option");
    const auto policy_ref = json_ref(approval.payload, "policy_ref");
    const auto candidate_ref = json_ref(approval.payload, "action_candidate_ref");
    const auto request_ref = json_ref(approval.payload, "request_ref");
    const auto verification_ref = json_ref(approval.payload, "verification_ref");

    const auto request_option_ref = json_ref(request->payload, "target_option");
    const auto request_policy_ref = json_ref(request->payload, "policy_ref");
    const auto request_candidate_ref = json_ref(request->payload, "action_candidate_ref");
    const auto request_verification_ref = json_ref(request->payload, "verification_ref");

    if (!ref_matches(request_ref, std::optional<arcs::verification::ArtifactRef>{{request->artifact_id, request->version_id}})) {
        return fail_state("resume.request_ref_mismatch", "approval request_ref does not match stored approval_request");
    }

    if (!ref_matches(verification_ref, std::optional<arcs::verification::ArtifactRef>{{verification_report->artifact_id, verification_report->version_id}})) {
        return fail_state("resume.verification_ref_mismatch", "approval verification_ref does not match stored verification report");
    }

    if (!ref_matches(option_ref, request_option_ref)) {
        return fail_state("resume.option_binding_mismatch", "approval_request target_option does not match approval");
    }

    if (!ref_matches(policy_ref, request_policy_ref)) {
        return fail_state("resume.policy_binding_mismatch", "approval_request policy_ref does not match approval");
    }

    if (!ref_matches(candidate_ref, request_candidate_ref)) {
        return fail_state("resume.action_candidate_binding_mismatch", "approval_request action_candidate_ref does not match approval");
    }

    if (!ref_matches(verification_ref, request_verification_ref)) {
        return fail_state("resume.verification_binding_mismatch", "approval_request verification_ref does not match approval");
    }

    if (request->payload.value("risk_summary", std::string{}) != approval.payload.value("risk_summary", std::string{})) {
        return fail_state("resume.risk_summary_mismatch", "approval_request risk_summary does not match approval");
    }

    if (request->payload.value("requested_scope", std::string{}) != approval.payload.value("approval_scope", std::string{})) {
        return fail_state("resume.scope_mismatch", "approval_request scope does not match approval");
    }

    arcs::verification::VerificationReportData report{};
    try {
        report = verification_report->payload.get<arcs::verification::VerificationReportData>();
    } catch (...) {
        return fail_state("resume.verification_report_parse_failed", "verification report invalid");
    }

    if (report.status != arcs::verification::CheckStatus::Pass) {
        return fail_state("resume.verification_not_pass", "verification report is not pass");
    }

    if (report.target.artifact_id != option->artifact_id || report.target.version_id != option->version_id) {
        return fail_state("resume.verification_target_mismatch", "verification report target does not match option");
    }

    if (approval.payload.value("approval_scope", std::string{}) != option->stream_key) {
        return fail_state("resume.approval_scope_invalid", "approval scope does not match option stream");
    }

    ApprovalResumeState state{};
    state.ok = true;
    state.error_code = "resume.ok";
    state.option = *option;
    state.policy = *policy;
    state.action_candidate = *candidate;
    state.approval_request = *request;
    state.verification_report_artifact = *verification_report;
    state.option_report = std::move(report);
    return state;
}

} // namespace arcs::core::resume
