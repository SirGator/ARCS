/**
 * @file llm_adapter.hpp
 * @brief Defines the interface for adapters that build requests to a
 *        language model, invoke it, and submit its validated response
 *        into ARCS.
 */

#pragma once

#include <nlohmann/json.hpp>

#include "adapters/common.hpp"

namespace arcs::adapters::llm {

/**
 * @brief Abstract interface for adapters that mediate between ARCS and an
 *        underlying LLM: constructing the model request, invoking the
 *        model, and validating its response for higher-level adapters.
 */
class ILlmAdapter : public arcs::adapters::IAdapter {
public:
    ~ILlmAdapter() override = default;

    /**
     * @brief Builds the raw request payload to send to the model.
     * @param prompt The prompt text to send to the model.
     * @param schema The schema the model's response is expected to conform to.
     * @param context Additional contextual data to include in the request.
     * @return The raw model request payload.
     */
    virtual nlohmann::json build_model_request(
        const std::string& prompt,
        const nlohmann::json& schema,
        const nlohmann::json& context) const = 0;

    /**
     * @brief Invokes the model with the given request and returns its output.
     * @param request The raw model request payload.
     * @return The model's response as a draft artifact.
     */
    virtual DraftArtifact call_model(const nlohmann::json& request) = 0;

    virtual LocalValidationResult validate_local(const DraftArtifact& response) const = 0;
};

} // namespace arcs::adapters::llm
