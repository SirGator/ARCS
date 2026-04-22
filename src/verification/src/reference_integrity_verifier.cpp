#include "verification/verifier.hpp"

#include "store/store.hpp"

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

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
