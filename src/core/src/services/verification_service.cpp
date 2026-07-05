/**
 * @file verification_service.cpp
 * @brief Implements VerificationService, the pipeline stage that verifies a proposed
 *        action against policy-defined verifier rules.
 *
 * Responsibilities: extracting the list of verifier rule names from a policy artifact,
 * building a VerificationPlan from those rules, running the core verification engine
 * (schema, reference integrity, permission, approval, authority) against an action, and
 * wrapping the resulting VerificationReportData into a named verification report artifact.
 */
#include "core/services/verification_service.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>

#include "verification/authority_verifier.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief Formats the current UTC time as an ISO-8601 timestamp string ("%Y-%m-%dT%H:%M:%SZ").
 * @return The current UTC timestamp string.
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

/**
 * @brief Appends the string entries found under a given key of a verifier_rules JSON
 *        object to the names vector; does nothing if the key is missing or not an array.
 * @param names Output vector to append matching rule names to.
 * @param verifier_rules JSON object holding verifier rule lists (e.g. hard/soft checks).
 * @param key Key within verifier_rules whose array entries should be appended.
 */
void append_rule_names(
    std::vector<std::string>& names,
    const nlohmann::json& verifier_rules,
    const char* key)
{
    if (!verifier_rules.contains(key) || !verifier_rules.at(key).is_array()) {
        return;
    }

    for (const auto& entry : verifier_rules.at(key)) {
        if (entry.is_string()) {
            names.push_back(entry.get<std::string>());
        }
    }
}

} // namespace

/**
 * @brief Extracts the combined list of hard and soft verifier check names declared in a
 *        policy artifact's "verifier_rules" payload section.
 * @param policy The policy artifact to read verifier rules from.
 * @return The list of verifier rule names (empty if the payload lacks a valid
 *         verifier_rules object).
 */
std::vector<std::string> VerificationService::verifier_rule_names(
    const arcs::artifact::ArtifactVersion& policy) const
{
    std::vector<std::string> names;

    if (!policy.payload.is_object() || !policy.payload.contains("verifier_rules")) {
        return names;
    }

    const auto& verifier_rules = policy.payload.at("verifier_rules");
    if (!verifier_rules.is_object()) {
        return names;
    }

    append_rule_names(names, verifier_rules, "hard_checks");
    append_rule_names(names, verifier_rules, "soft_checks");
    return names;
}

/**
 * @brief Builds a verification plan from the core verifier registry, restricted to the
 *        verifier rule names declared in the given policy.
 * @param policy The policy artifact whose verifier rules define the plan.
 * @return The resulting VerificationPlan.
 */
arcs::verification::VerificationPlan VerificationService::build_policy_plan(
    const arcs::artifact::ArtifactVersion& policy) const
{
    return arcs::verification::VerifierRegistry::with_core_verifiers().build_plan(verifier_rule_names(policy));
}

/**
 * @brief Appends a check entry with Unknown status for each verifier name that the
 *        policy requested but that the core verification flow does not support.
 * @param report Verification report to append the unsupported-check entries to.
 * @param unsupported_checks Names of unsupported checks requested by the policy.
 */
void VerificationService::append_unsupported_checks(
    arcs::verification::VerificationReportData& report,
    const std::vector<std::string>& unsupported_checks) const
{
    for (const auto& check_name : unsupported_checks) {
        report.checks.push_back(arcs::verification::VerificationCheck{
            .name = check_name,
            .status = arcs::verification::CheckStatus::Unknown,
            .detail = "policy requested unsupported verifier in core flow",
        });
    }
}

/**
 * @brief Runs the full core verification engine (schema, reference integrity,
 *        permission, approval, and authority verifiers) against an action artifact.
 * @param action The action artifact being verified.
 * @param context Verification context (e.g. permissions, policy state) used by verifiers.
 * @return The aggregated VerificationReportData from all core verifiers.
 */
arcs::verification::VerificationReportData VerificationService::verify_action(
    const arcs::artifact::ArtifactVersion& action,
    const arcs::verification::VerificationContext& context) const
{
    arcs::verification::VerificationEngine engine;
    engine.add_verifier(std::make_shared<arcs::verification::SchemaVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ReferenceIntegrityVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::PermissionVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ApprovalVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::AuthorityVerifier>());
    return engine.run_all(action, context);
}

/**
 * @brief Wraps a verification report into a system-authored, high-trust verification
 *        report artifact with an explicit artifact/version id, targeting the same
 *        stream as the verified target.
 * @param target The artifact that was verified.
 * @param report The verification report data to embed in the artifact.
 * @param artifact_id Explicit artifact id to assign to the report artifact.
 * @param version_id Explicit version id to assign to the report artifact.
 * @return The constructed verification report artifact version.
 */
arcs::artifact::ArtifactVersion VerificationService::make_named_report_artifact(
    const arcs::artifact::ArtifactVersion& target,
    const arcs::verification::VerificationReportData& report,
    const std::string& artifact_id,
    const std::string& version_id) const
{
    return arcs::verification::make_verification_report_artifact(
        target,
        report,
        arcs::artifact::ActorRef{.actor_type = "system", .id = "kernel"},
        arcs::artifact::SourceRef{.kind = "internal", .ref = "verification"},
        arcs::artifact::TrustInfo{.level = "high", .source_class = "system"},
        artifact_id,
        version_id,
        target.stream_key,
        utc_now());
}

} // namespace arcs::core::services
