#include "verification/verifier.hpp"

#include <sstream>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

std::vector<std::string> required_permissions_from_payload(const ArtifactVersion& target) {
    std::vector<std::string> out;

    if (!target.payload.is_object()) {
        return out;
    }

    if (!target.payload.contains("requires_permissions")) {
        return out;
    }

    const auto& value = target.payload.at("requires_permissions");
    if (!value.is_array()) {
        return out;
    }

    for (const auto& entry : value) {
        if (entry.is_string()) {
            out.push_back(entry.get<std::string>());
        }
    }

    return out;
}

std::string join_missing(const std::vector<std::string>& missing) {
    std::ostringstream oss;
    for (std::size_t i = 0; i < missing.size(); ++i) {
        if (i != 0) {
            oss << ", ";
        }
        oss << missing[i];
    }
    return oss.str();
}

} // namespace

VerificationCheck PermissionVerifier::check(
    const ArtifactVersion& target,
    const VerificationContext& context) const {
    VerificationCheck result{};
    result.name = "permission";
    result.status = CheckStatus::Pass;

    const auto required = required_permissions_from_payload(target);

    if (required.empty()) {
        result.detail = "no required permissions";
        return result;
    }

    std::vector<std::string> missing;
    missing.reserve(required.size());

    for (const auto& capability : required) {
        if (!context.permissions.has_capability(capability)) {
            missing.push_back(capability);
        }
    }

    if (!missing.empty()) {
        result.status = CheckStatus::Fail;
        if (missing.size() == 1) {
            result.detail = "capability " + missing.front() + " fehlt";
        } else {
            result.detail = "capabilities fehlen: " + join_missing(missing);
        }
        return result;
    }

    result.detail = "all required permissions present";
    return result;
}

} // namespace arcs::verification
