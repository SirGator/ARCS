/**
 * @file output_adapter.hpp
 * @brief Defines the interface for adapters that render an artifact into
 *        an output payload and deliver it to an external destination.
 */

#pragma once

#include <string>

#include <nlohmann/json.hpp>

#include "adapters/common.hpp"

namespace arcs::adapters::output {

/**
 * @brief Abstract interface for adapters that build an output payload from
 *        an artifact, validate it locally, and deliver it externally.
 */
class IOutputAdapter : public arcs::adapters::IAdapter {
public:
    ~IOutputAdapter() override = default;

    /**
     * @brief Builds the output payload to be delivered from an artifact.
     * @param artifact The draft artifact to render.
     * @return The raw output payload.
     */
    virtual nlohmann::json build_output(const DraftArtifact& artifact) const = 0;

    /**
     * @brief Performs local validation on a built payload before delivery.
     * @param payload The raw output payload to validate.
     * @return The result of the local validation.
     */
    virtual LocalValidationResult validate_local(const nlohmann::json& payload) const = 0;

    /**
     * @brief Delivers a validated payload to its external destination.
     * @param payload The raw output payload to deliver.
     * @return Whether the delivery was accepted, including diagnostics.
     */
    virtual CoreSubmissionResult deliver(const nlohmann::json& payload) = 0;
};

} // namespace arcs::adapters::output
