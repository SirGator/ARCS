/**
 * @file reasoning_adapter.hpp
 * @brief Defines the interface for adapters that turn a reasoning request
 *        into a proposal artifact and submit it to the core.
 */

#pragma once

#include <nlohmann/json.hpp>

#include "adapters/common.hpp"

namespace arcs::adapters::reasoning {

/**
 * @brief Abstract interface for adapters that generate proposals in
 *        response to reasoning requests as core intake submissions.
 */
class IReasoningAdapter : public arcs::adapters::IAdapter {
public:
    ~IReasoningAdapter() override = default;

    /**
     * @brief Translates a draft artifact request into the adapter's raw
     *        reasoning-request representation.
     * @param request The draft artifact describing the reasoning request.
     * @return The raw reasoning request.
     */
    virtual nlohmann::json receive_reasoning_request(const DraftArtifact& request) const = 0;

    /**
     * @brief Generates a proposal artifact from a raw reasoning request.
     * @param request The raw reasoning request to act on.
     * @return The generated proposal as a draft artifact.
     */
    virtual DraftArtifact generate_proposal(const nlohmann::json& request) = 0;

};

} // namespace arcs::adapters::reasoning
