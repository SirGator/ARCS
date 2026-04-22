#include "verification/verifier.hpp"

#include <string>

namespace arcs {

VerificationCheck ApprovalVerifier::check(
    const ArtifactVersion& target,
    const VerificationContext& context) const {
    VerificationCheck result{};
    result.name = "approval";

    if (target.type != "approval") {
        result.status = CheckStatus::Pass;
        result.detail = "not an approval artifact";
        return result;
    }

    if (context.policy == nullptr) {
        result.status = CheckStatus::Unknown;
        result.detail = "policy missing in verification context";
        return result;
    }

    if (!target.payload.is_object() || !target.payload.contains("decision") ||
        !target.payload.at("decision").is_string()) {
        result.status = CheckStatus::Unknown;
        result.detail = "approval decision missing";
        return result;
    }

    const auto decision = target.payload.at("decision").get<std::string>();
    if (decision == "approve") {
        result.status = CheckStatus::Pass;
        result.detail = "approval granted";
        return result;
    }

    result.status = CheckStatus::Fail;
    result.detail = "approval decision is " + decision;
    return result;
}

} // namespace arcs
