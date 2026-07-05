/**
 * @file decision_service.cpp
 * @brief Implements DecisionService, the pipeline stage responsible for recording
 *        the kernel's final decision for a given option: a "decision" artifact that
 *        captures the outcome status/reason together with references to the
 *        verification report and, where applicable, the approval, action, and
 *        execution result artifacts involved.
 */

#include "core/services/decision_service.hpp"

#include "artifact/factory.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief Returns the current UTC time formatted as an ISO-8601 string (YYYY-MM-DDTHH:MM:SSZ).
 * @return The formatted current UTC timestamp.
 */
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

} // namespace

/**
 * @brief Builds a "decision" artifact recording the pipeline outcome for an option,
 *        including its status, reason, verification report reference, and the
 *        associated approval/action/execution result artifact IDs.
 * @param option The decision option artifact this decision is about.
 * @param report The verification report data providing target and status references.
 * @param status The decision outcome status (e.g. approved/rejected).
 * @param reason A human-readable explanation for the decision.
 * @param approval_artifact_id The ID of the approval artifact involved, if any.
 * @param action_artifact_id The ID of the action artifact involved, if any.
 * @param execution_result_artifact_id The ID of the execution result artifact involved, if any.
 * @return The constructed decision artifact version.
 */
arcs::artifact::ArtifactVersion DecisionService::make_decision(
    const arcs::artifact::ArtifactVersion& option,
    const arcs::verification::VerificationReportData& report,
    const std::string& status,
    const std::string& reason,
    const std::string& approval_artifact_id,
    const std::string& action_artifact_id,
    const std::string& execution_result_artifact_id) const
{
    auto artifact = arcs::artifact::factory::make_base_artifact(
        "decision",
        "arcs.decision.v1",
        option.stream_key,
        "system",
        "kernel",
        "internal",
        "decision",
        "high",
        "system",
        utc_now());

    artifact.payload = nlohmann::json{
        {"status", status},
        {"reason", reason},
        {"verification_report", {
            {"artifact_id", report.target.artifact_id},
            {"version_id", report.target.version_id},
            {"status", arcs::verification::to_string(report.status)},
        }},
        {"approval_artifact_id", approval_artifact_id},
        {"action_artifact_id", action_artifact_id},
        {"execution_result_artifact_id", execution_result_artifact_id},
    };
    artifact.provenance.parents = {option.artifact_id};
    artifact.provenance.rules_applied = {"kernel_decision"};
    artifact.provenance.transform = "decide";
    return artifact;
}

} // namespace arcs::core::services
