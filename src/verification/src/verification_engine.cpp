#include "verification/verifier.hpp"

#include <algorithm>
#include <stdexcept>
#include <utility>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

bool has_status(const std::vector<VerificationCheck>& checks, CheckStatus wanted) {
    return std::any_of(
        checks.begin(),
        checks.end(),
        [&](const VerificationCheck& check) { return check.status == wanted; });
}

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

bool EffectivePermissions::has_capability(const std::string& capability) const {
    return std::find(capabilities.begin(), capabilities.end(), capability) != capabilities.end();
}

bool EffectivePermissions::has_scope(const std::string& scope) const {
    return std::find(scopes.begin(), scopes.end(), scope) != scopes.end();
}

} // namespace arcs::reducer

namespace arcs::verification {

void VerificationEngine::add_verifier(std::shared_ptr<IVerifier> verifier) {
    if (!verifier) {
        throw std::invalid_argument("VerificationEngine::add_verifier received null");
    }
    verifiers_.push_back(std::move(verifier));
}

CheckStatus aggregate_status(const std::vector<VerificationCheck>& checks) {
    if (has_status(checks, CheckStatus::Fail)) {
        return CheckStatus::Fail;
    }
    if (has_status(checks, CheckStatus::Unknown)) {
        return CheckStatus::Unknown;
    }
    return CheckStatus::Pass;
}

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
    ArtifactVersion artifact{};
    artifact.artifact_id = artifact_id;
    artifact.version_id = version_id;
    artifact.version = 1;
    artifact.type = "verification_report";
    artifact.schema_id = "arcs.verification_report.v1";
    artifact.schema_version = 1;
    artifact.created_at = created_at;
    artifact.created_by = created_by;
    artifact.source = source;
    artifact.trust = trust;
    artifact.stream_key = stream_key;
    artifact.payload = nlohmann::json(report);

    artifact.provenance.parents.push_back(target.artifact_id);
    artifact.provenance.rules_applied.push_back("verification_engine");
    artifact.provenance.transform = "verify";

    return artifact;
}

void to_json(nlohmann::json& j, const ArtifactRef& ref) {
    j = nlohmann::json{
        {"artifact_id", ref.artifact_id},
        {"version_id", ref.version_id},
    };
}

void from_json(const nlohmann::json& j, ArtifactRef& ref) {
    j.at("artifact_id").get_to(ref.artifact_id);
    j.at("version_id").get_to(ref.version_id);
}

void to_json(nlohmann::json& j, const VerificationCheck& check) {
    j = nlohmann::json{
        {"name", check.name},
        {"status", to_string(check.status)},
        {"detail", check.detail},
    };
}

void from_json(const nlohmann::json& j, VerificationCheck& check) {
    j.at("name").get_to(check.name);
    check.status = check_status_from_string(j.at("status").get<std::string>());
    if (j.contains("detail")) {
        j.at("detail").get_to(check.detail);
    } else {
        check.detail.clear();
    }
}

void to_json(nlohmann::json& j, const VerificationReportData& report) {
    j = nlohmann::json{
        {"target", report.target},
        {"status", to_string(report.status)},
        {"checks", report.checks},
        {"blockers", report.blockers},
        {"recommendations", report.recommendations},
    };
}

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
