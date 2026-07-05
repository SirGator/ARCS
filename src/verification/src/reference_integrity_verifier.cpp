/**
 * @file reference_integrity_verifier.cpp
 * @brief Implements ReferenceIntegrityVerifier::check, which walks a
 *        target's payload for embedded artifact/version references and
 *        confirms each one resolves to a matching version in the store.
 */
#include "verification/verifier.hpp"

#include "store/store.hpp"

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

/**
 * @brief Recursively walks a JSON value, collecting an ArtifactRef for
 *        every object encountered that has both "artifact_id" and
 *        "version_id" string fields.
 * @param value JSON value (object, array, or scalar) to scan.
 * @param out Output vector that collected references are appended to.
 */
void collect_refs_from_json(const nlohmann::json& value,
                            std::vector<ArtifactRef>& out) {
    if (value.is_object()) {
        const bool has_artifact_id =
            value.contains("artifact_id") && value.at("artifact_id").is_string();
        const bool has_version_id =
            value.contains("version_id") && value.at("version_id").is_string();

        if (has_artifact_id && has_version_id) {
            out.push_back(ArtifactRef{
                .artifact_id = value.at("artifact_id").get<std::string>(),
                .version_id = value.at("version_id").get<std::string>(),
            });
        }

        for (const auto& [key, child] : value.items()) {
            (void)key;
            collect_refs_from_json(child, out);
        }
        return;
    }

    if (value.is_array()) {
        for (const auto& child : value) {
            collect_refs_from_json(child, out);
        }
    }
}

/**
 * @brief Collects all artifact/version references embedded anywhere in a
 *        target's payload.
 * @param target Artifact version whose payload is scanned.
 * @return The list of ArtifactRef values found.
 */
std::vector<ArtifactRef> collect_all_refs(const ArtifactVersion& target) {
    std::vector<ArtifactRef> refs;

    // Alle payload-Refs durchsuchen
    collect_refs_from_json(target.payload, refs);

    // Falls du später Provenance-Refs typisiert speicherst,
    // kannst du sie hier zusätzlich einsammeln.
    // Im Moment bleibt es bewusst klein für Phase 5.

    return refs;
}

} // namespace

/**
 * @brief Collects every artifact reference in the target's payload and
 *        confirms each resolves in the store to a version belonging to
 *        the expected artifact_id. Targets with no references pass
 *        trivially.
 * @param target Artifact version being verified.
 * @param context Verification context; must provide a store.
 * @return VerificationCheck named "reference_integrity" with the outcome.
 */
VerificationCheck ReferenceIntegrityVerifier::check(
    const ArtifactVersion& target,
    const VerificationContext& context) const {
    VerificationCheck result{};
    result.name = "reference_integrity";
    result.status = CheckStatus::Pass;

    const auto refs = collect_all_refs(target);
    if (refs.empty()) {
        result.detail = "no references to validate";
        return result;
    }

    if (context.store == nullptr) {
        result.status = CheckStatus::Unknown;
        result.detail = "store missing in verification context";
        return result;
    }

    for (const auto& ref : refs) {
        try {
            const auto resolved = context.store->get_version(ref.version_id);

            if (resolved.artifact_id != ref.artifact_id) {
                result.status = CheckStatus::Fail;
                result.detail =
                    "reference mismatch: version_id " + ref.version_id +
                    " belongs to artifact_id " + resolved.artifact_id +
                    ", expected " + ref.artifact_id;
                return result;
            }
        } catch (const std::exception&) {
            result.status = CheckStatus::Fail;
            result.detail =
                "missing reference: artifact_id=" + ref.artifact_id +
                ", version_id=" + ref.version_id;
            return result;
        }
    }

    result.detail = "all references resolved";
    return result;
}

} // namespace arcs::verification
