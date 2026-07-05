/**
 * @file schema_verifier.cpp
 * @brief Implements SchemaVerifier::check, which validates a target's
 *        payload against its declared JSON schema via the schema registry.
 */
#include "verification/verifier.hpp"

#include "schema/validator.hpp"

#include <string>

namespace arcs::verification {

/**
 * @brief Validates the target's payload against the schema identified by
 *        target.schema_id, using the schema registry from the context.
 *        Fails if schema_id is empty, validation fails, or an exception
 *        is thrown during validation.
 * @param target Artifact version being verified.
 * @param context Verification context; must provide a schema registry.
 * @return VerificationCheck named "schema" with the outcome.
 */
VerificationCheck SchemaVerifier::check(
    const ArtifactVersion& target,
    const VerificationContext& context) const {
    VerificationCheck result{};
    result.name = "schema";
    result.status = CheckStatus::Pass;

    if (context.schema_registry == nullptr) {
        result.status = CheckStatus::Unknown;
        result.detail = "schema registry missing in verification context";
        return result;
    }

    if (target.schema_id.empty()) {
        result.status = CheckStatus::Fail;
        result.detail = "schema_id missing";
        return result;
    }

    try {
        const auto validation = arcs::schema::Validator::validate(
            target.payload,
            target.schema_id,
            *context.schema_registry);

        if (!validation.valid) {
            result.status = CheckStatus::Fail;

            if (!validation.errors.empty()) {
                const auto& first = validation.errors.front();
                if (!first.path.empty() && !first.message.empty()) {
                    result.detail = first.path + ": " + first.message;
                } else if (!first.message.empty()) {
                    result.detail = first.message;
                } else {
                    result.detail = "schema validation failed";
                }
            } else {
                result.detail = "schema validation failed";
            }

            return result;
        }
    } catch (const std::exception& ex) {
        result.status = CheckStatus::Fail;
        result.detail = std::string("schema validation exception: ") + ex.what();
        return result;
    } catch (...) {
        result.status = CheckStatus::Fail;
        result.detail = "schema validation exception";
        return result;
    }

    result.detail = "schema valid";
    return result;
}

} // namespace arcs::verification
