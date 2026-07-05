/**
 * @file scope_verifier.cpp
 * @brief Implements ScopeVerifier::check, which confirms that a target's
 *        required scopes (if declared) match its stream-derived scope and
 *        are within the acting principal's allowed permission scopes.
 */
#include "verification/verifier.hpp"

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::verification {

namespace {

/**
 * @brief Reads the "required_scopes" string array from a target's payload.
 * @param target Artifact version whose payload is inspected.
 * @return The list of required scope strings, empty if none declared.
 */
std::vector<std::string> required_scopes_from_payload(const ArtifactVersion& target) {
    std::vector<std::string> out;

    if (!target.payload.is_object()) {
        return out;
    }

    if (!target.payload.contains("required_scopes")) {
        return out;
    }

    const auto& value = target.payload.at("required_scopes");
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
 * @brief Derives the scope associated with an artifact's stream from its
 *        stream key. Currently the stream key is used directly as the scope.
 * @param stream_key Stream key of the target artifact.
 * @return The derived scope string, empty if stream_key is empty.
 */
std::string stream_scope_from_stream_key(const std::string& stream_key) {
    if (stream_key.empty()) {
        return {};
    }
    return stream_key;
}

} // namespace

/**
 * @brief Verifies that a target's required scopes (if any) match the
 *        scope derived from its stream key and are permitted by the
 *        context's effective permission scopes. Targets declaring no
 *        required scopes pass trivially; ambiguous cases (unresolvable
 *        scope, empty required scope, or no permission scopes available
 *        to disambiguate) report Unknown rather than guessing.
 * @param target Artifact version being verified.
 * @param context Verification context, including effective permissions.
 * @return VerificationCheck named "scope" with the outcome.
 */
VerificationCheck ScopeVerifier::check(
    const ArtifactVersion& target,
    const VerificationContext& context) const {
    VerificationCheck result{};
    result.name = "scope";
    result.status = CheckStatus::Pass;

    const auto required_scopes = required_scopes_from_payload(target);

    // V1-Regel:
    // - keine Scope-Anforderung -> pass
    // - Scope-Anforderung vorhanden, aber nicht eindeutig prüfbar -> unknown
    // - Scope-Anforderung vorhanden und eindeutig, aber nicht erlaubt -> fail
    // - Scope-Anforderung vorhanden und erlaubt -> pass

    if (required_scopes.empty()) {
        result.detail = "no required scopes";
        return result;
    }

    const auto derived_scope = stream_scope_from_stream_key(target.stream_key);
    if (derived_scope.empty()) {
        result.status = CheckStatus::Unknown;
        result.detail = "scope not resolvable from stream_key";
        return result;
    }

    bool matched_any = false;
    for (const auto& required_scope : required_scopes) {
        if (required_scope.empty()) {
            result.status = CheckStatus::Unknown;
            result.detail = "empty required scope";
            return result;
        }

        if (required_scope == derived_scope) {
            matched_any = true;
            break;
        }
    }

    if (!matched_any) {
        // Wenn Permissions gar keine Scopes tragen, behandeln wir das in V1
        // als unzureichend bestimmbar statt stillschweigend fail.
        if (context.permissions.scopes.empty()) {
            result.status = CheckStatus::Unknown;
            result.detail = "required scope does not match target scope and no permission scopes are available";
            return result;
        }

        for (const auto& allowed_scope : context.permissions.scopes) {
            if (allowed_scope == derived_scope) {
                result.status = CheckStatus::Fail;
                result.detail = "target scope is allowed, but required_scopes in payload do not match stream scope";
                return result;
            }
        }

        result.status = CheckStatus::Fail;
        result.detail = "scope outside allowed permissions: " + derived_scope;
        return result;
    }

    if (!context.permissions.scopes.empty() &&
        !context.permissions.has_scope(derived_scope)) {
        result.status = CheckStatus::Fail;
        result.detail = "scope outside allowed permissions: " + derived_scope;
        return result;
    }

    result.detail = "scope valid: " + derived_scope;
    return result;
}

} // namespace arcs::verification
