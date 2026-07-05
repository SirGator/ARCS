/**
 * @file input_adapter.hpp
 * @brief Defines the interface for adapters that ingest external signals
 *        into ARCS, normalizing and validating them before submission.
 */

#pragma once

#include <string>

#include <nlohmann/json.hpp>

#include "adapters/common.hpp"

namespace arcs::adapters::input {

/**
 * @brief Abstract interface for adapters that receive external signals,
 *        normalize them into adapter submissions for the core intake layer.
 */
class IInputAdapter : public arcs::adapters::IAdapter {
public:
    ~IInputAdapter() override = default;

    /**
     * @brief Receives the next raw signal from the external source.
     * @return The raw external signal.
     */
    virtual nlohmann::json receive_external_signal() = 0;

    /**
     * @brief Normalizes a raw external signal into a draft artifact.
     * @param signal The raw signal to normalize.
     * @return The normalized draft artifact.
     */
    virtual DraftArtifact normalize_signal(const nlohmann::json& signal) = 0;

};

} // namespace arcs::adapters::input
