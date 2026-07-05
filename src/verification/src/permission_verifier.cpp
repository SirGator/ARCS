/**
 * @file permission_verifier.cpp
 * @brief Implements PermissionVerifier::check, which compares the
 *        capabilities required by a target's payload against the acting
 *        principal's effective permissions.
 */
#include "verification/verifier.hpp"

#include <sstream>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

/**
 * @brief Reads the list of required capability names from a target's
 *        payload, checking either "requires_permissions" or
 *        "required_permissions" (whichever is present).
 * @param target Artifact version whose payload is inspected.
 * @return The list of required capability strings, empty if none declared.
 */
std::vector<std::string> required_permissions_from_payload(const ArtifactVersion& target) {
    std::vector<std::string> out;

    if (!target.payload.is_object()) {
        return out;
    }

    const char* field_name = nullptr;
    if (target.payload.contains("requires_permissions")) {
        field_name = "requires_permissions";
    } else if (target.payload.contains("required_permissions")) {
        field_name = "required_permissions";
    }

    if (field_name == nullptr) {
        return out;
    }

    const auto& value = target.payload.at(field_name);
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

/**
 * @brief Joins a list of missing capability names into a single
 *        comma-separated string for use in a detail message.
 * @param missing Capability names that were missing.
 * @return The comma-separated string, empty if missing is empty.
 */
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

/**
 * @brief Verifies that the acting principal's effective permissions cover
 *        every capability the target declares as required. Targets that
 *        declare no required permissions pass trivially.
 * @param target Artifact version being verified.
 * @param context Verification context, including effective permissions.
 * @return VerificationCheck named "permission" with the outcome.
 */
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
