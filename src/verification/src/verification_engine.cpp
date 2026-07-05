/**
 * @file verification_engine.cpp
 * @brief Implements the core verification plumbing declared in
 *        verifier.hpp: CheckStatus (de)serialization, the
 *        EffectivePermissions helper methods, VerificationEngine,
 *        VerifierRegistry, report aggregation/construction, and the
 *        to_json/from_json overloads for the verification report types.
 */
#include "verification/verifier.hpp"

#include <algorithm>
#include <stdexcept>
#include <utility>

#include <nlohmann/json.hpp>

#include "artifact/factory.hpp"

namespace arcs::verification {

namespace {

/**
 * @brief Checks whether any check in a collection has the given status.
 * @param checks Checks to search.
 * @param wanted Status to look for.
 * @return True if at least one check has the wanted status.
 */
bool has_status(const std::vector<VerificationCheck>& checks, CheckStatus wanted) {
    return std::any_of(
        checks.begin(),
        checks.end(),
        [&](const VerificationCheck& check) { return check.status == wanted; });
}

/**
 * @brief Finds the first non-empty detail message among checks matching
 *        the given status.
 * @param checks Checks to search.
 * @param wanted Status to match.
 * @return The first matching non-empty detail, or an empty string if none.
 */
std::string first_non_empty_detail(const std::vector<VerificationCheck>& checks,
                                   CheckStatus wanted) {
    for (const auto& check : checks) {
        if (check.status == wanted && !check.detail.empty()) {
            return check.detail;
        }
    }
    return {};
}

} // namespace

/**
 * @brief Converts a CheckStatus to its lowercase string representation.
 * @param status Status value to convert.
 * @return "pass", "fail", or "unknown".
 * @throws std::invalid_argument if status is not a recognized enumerator.
 */
std::string to_string(CheckStatus status) {
    switch (status) {
        case CheckStatus::Pass:
            return "pass";
        case CheckStatus::Fail:
            return "fail";
        case CheckStatus::Unknown:
            return "unknown";
    }
    throw std::invalid_argument("unknown CheckStatus");
}

/**
 * @brief Parses a CheckStatus from its lowercase string representation.
 * @param value "pass", "fail", or "unknown".
 * @return The corresponding CheckStatus.
 * @throws std::invalid_argument if value is not a recognized status string.
 */
CheckStatus check_status_from_string(const std::string& value) {
    if (value == "pass") {
        return CheckStatus::Pass;
    }
    if (value == "fail") {
        return CheckStatus::Fail;
    }
    if (value == "unknown") {
        return CheckStatus::Unknown;
    }
    throw std::invalid_argument("invalid check status string: " + value);
}

} // namespace arcs::verification

namespace arcs::reducer {

/**
 * @brief Checks whether this set of effective permissions includes the
 *        given capability.
 * @param capability Capability identifier to search for.
 * @return True if the capability is present.
 */
bool EffectivePermissions::has_capability(const std::string& capability) const {
    return std::find(capabilities.begin(), capabilities.end(), capability) != capabilities.end();
}

/**
 * @brief Checks whether this set of effective permissions includes the
 *        given scope.
 * @param scope Scope identifier to search for.
 * @return True if the scope is present.
 */
bool EffectivePermissions::has_scope(const std::string& scope) const {
    return std::find(scopes.begin(), scopes.end(), scope) != scopes.end();
}

} // namespace arcs::reducer

namespace arcs::verification {

/**
 * @brief Adds a verifier to the engine's collection to be run by run_all.
 * @param verifier Verifier instance to add.
 * @throws std::invalid_argument if verifier is null.
 */
void VerificationEngine::add_verifier(std::shared_ptr<IVerifier> verifier) {
    if (!verifier) {
        throw std::invalid_argument("VerificationEngine::add_verifier received null");
    }
    verifiers_.push_back(std::move(verifier));
}

/**
 * @brief Registers a verifier factory under a check name.
 * @param check_name Unique, non-empty name of the check.
 * @param factory Non-null factory that constructs the verifier instance.
 * @throws std::invalid_argument if check_name is empty or factory is null.
 */
void VerifierRegistry::register_factory(const std::string& check_name, VerifierFactory factory)
{
    if (check_name.empty() || !factory) {
        throw std::invalid_argument("VerifierRegistry::register_factory received invalid input");
    }
    factories_[check_name] = std::move(factory);
}

/**
 * @brief Resolves each requested check name to a verifier via its
 *        registered factory, collecting unresolved names separately.
 * @param check_names Names of the checks to include in the plan.
 * @return A VerificationPlan with constructed verifiers and any
 *         unsupported check names.
 */
VerificationPlan VerifierRegistry::build_plan(const std::vector<std::string>& check_names) const
{
    VerificationPlan plan;
    for (const auto& check_name : check_names) {
        const auto it = factories_.find(check_name);
        if (it == factories_.end()) {
            plan.unsupported_checks.push_back(check_name);
            continue;
        }
        plan.verifiers.push_back(it->second());
    }
    return plan;
}

/**
 * @brief Builds a VerifierRegistry pre-populated with the built-in core
 *        verifiers: "permission", "scope", "approval", "policy_drift".
 * @return The populated VerifierRegistry.
 */
VerifierRegistry VerifierRegistry::with_core_verifiers()
{
    VerifierRegistry registry;
    registry.register_factory("permission", [] { return std::make_shared<PermissionVerifier>(); });
    registry.register_factory("scope", [] { return std::make_shared<ScopeVerifier>(); });
    registry.register_factory("approval", [] { return std::make_shared<ApprovalVerifier>(); });
    registry.register_factory("policy_drift", [] { return std::make_shared<PolicyHeadVerifier>(); });
    return registry;
}

/**
 * @brief Aggregates a set of individual checks into a single status:
 *        Fail if any check failed, else Unknown if any is undecided,
 *        else Pass.
 * @param checks Individual verification checks to aggregate.
 * @return The aggregated CheckStatus.
 */
CheckStatus aggregate_status(const std::vector<VerificationCheck>& checks) {
    if (has_status(checks, CheckStatus::Fail)) {
        return CheckStatus::Fail;
    }
    if (has_status(checks, CheckStatus::Unknown)) {
        return CheckStatus::Unknown;
    }
    return CheckStatus::Pass;
}

/**
 * @brief Builds a VerificationReportData from a target and a list of
 *        checks: sets the target reference, computes the aggregate
 *        status, and populates the blockers list with details of any
 *        failing or unknown checks.
 * @param target Artifact version the checks were run against.
 * @param checks Individual verification checks to include (moved from).
 * @return The assembled VerificationReportData.
 */
VerificationReportData make_verification_report(
    const ArtifactVersion& target,
    std::vector<VerificationCheck> checks) {
    VerificationReportData report{};
    report.target = ArtifactRef{
        .artifact_id = target.artifact_id,
        .version_id = target.version_id,
    };
    report.checks = std::move(checks);
    report.status = aggregate_status(report.checks);

    if (report.status == CheckStatus::Fail) {
        for (const auto& check : report.checks) {
            if (check.status == CheckStatus::Fail) {
                if (!check.detail.empty()) {
                    report.blockers.push_back(check.name + ": " + check.detail);
                } else {
                    report.blockers.push_back(check.name + " failed");
                }
            }
        }
    }

    if (report.status == CheckStatus::Unknown) {
        const auto detail = first_non_empty_detail(report.checks, CheckStatus::Unknown);
        if (!detail.empty()) {
            report.blockers.push_back("unknown: " + detail);
        } else {
            report.blockers.push_back("unknown: verification could not be decided deterministically");
        }
    }

    return report;
}

/**
 * @brief Runs every registered verifier against the target, in
 *        registration order, and aggregates the resulting checks into a
 *        report.
 * @param target Artifact version being verified.
 * @param context Verification context shared by all verifiers.
 * @return The aggregated VerificationReportData.
 */
VerificationReportData VerificationEngine::run_all(
    const ArtifactVersion& target,
    const VerificationContext& context) const {
    std::vector<VerificationCheck> checks;
    checks.reserve(verifiers_.size());

    for (const auto& verifier : verifiers_) {
        checks.push_back(verifier->check(target, context));
    }

    return make_verification_report(target, std::move(checks));
}

/**
 * @brief Wraps a VerificationReportData as a standalone
 *        "verification_report" artifact, using the factory to build the
 *        base artifact and attaching provenance linking it back to the
 *        verified target.
 * @param target Artifact version that was verified.
 * @param report Verification report payload to embed as the artifact's
 *               payload.
 * @param created_by Actor that produced the report.
 * @param source Source reference describing where the report originated.
 * @param trust Trust metadata to attach to the new artifact.
 * @param artifact_id Identifier to assign to the new artifact.
 * @param version_id Version identifier to assign to the new artifact.
 * @param stream_key Stream key to assign to the new artifact.
 * @param created_at Creation timestamp to assign to the new artifact.
 * @return The constructed verification report ArtifactVersion.
 */
ArtifactVersion make_verification_report_artifact(
    const ArtifactVersion& target,
    const VerificationReportData& report,
    const ActorRef& created_by,
    const SourceRef& source,
    const TrustInfo& trust,
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& stream_key,
    const std::string& created_at) {
    ArtifactVersion artifact = arcs::artifact::factory::make_base_artifact(
        "verification_report",
        "arcs.verification_report.v1",
        stream_key,
        created_by.actor_type,
        created_by.id,
        source.kind,
        source.ref,
        trust.level,
        trust.source_class,
        created_at);
    artifact.artifact_id = artifact_id;
    artifact.version_id = version_id;
    artifact.payload = nlohmann::json(report);

    artifact.provenance.parents.push_back(target.artifact_id);
    artifact.provenance.rules_applied.push_back("verification_engine");
    artifact.provenance.transform = "verify";

    return artifact;
}

/** @brief Serializes an ArtifactRef to JSON. */
void to_json(nlohmann::json& j, const ArtifactRef& ref) {
    j = nlohmann::json{
        {"artifact_id", ref.artifact_id},
        {"version_id", ref.version_id},
    };
}

/** @brief Deserializes an ArtifactRef from JSON. */
void from_json(const nlohmann::json& j, ArtifactRef& ref) {
    j.at("artifact_id").get_to(ref.artifact_id);
    j.at("version_id").get_to(ref.version_id);
}

/** @brief Serializes a VerificationCheck to JSON. */
void to_json(nlohmann::json& j, const VerificationCheck& check) {
    j = nlohmann::json{
        {"name", check.name},
        {"status", to_string(check.status)},
        {"detail", check.detail},
    };
}

/** @brief Deserializes a VerificationCheck from JSON. */
void from_json(const nlohmann::json& j, VerificationCheck& check) {
    j.at("name").get_to(check.name);
    check.status = check_status_from_string(j.at("status").get<std::string>());
    if (j.contains("detail")) {
        j.at("detail").get_to(check.detail);
    } else {
        check.detail.clear();
    }
}

/** @brief Serializes a VerificationReportData to JSON. */
void to_json(nlohmann::json& j, const VerificationReportData& report) {
    j = nlohmann::json{
        {"target", report.target},
        {"status", to_string(report.status)},
        {"checks", report.checks},
        {"blockers", report.blockers},
        {"recommendations", report.recommendations},
    };
}

/** @brief Deserializes a VerificationReportData from JSON. */
void from_json(const nlohmann::json& j, VerificationReportData& report) {
    j.at("target").get_to(report.target);
    report.status = check_status_from_string(j.at("status").get<std::string>());
    j.at("checks").get_to(report.checks);

    if (j.contains("blockers")) {
        j.at("blockers").get_to(report.blockers);
    } else {
        report.blockers.clear();
    }

    if (j.contains("recommendations")) {
        j.at("recommendations").get_to(report.recommendations);
    } else {
        report.recommendations.clear();
    }
}

} // namespace arcs::verification
