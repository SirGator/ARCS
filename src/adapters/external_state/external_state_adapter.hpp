/**
 * @file external_state_adapter.hpp
 * @brief Defines the interface for adapters that observe state changes in
 *        external systems and bring them into ARCS as validated artifacts.
 */

#pragma once

#include <nlohmann/json.hpp>

#include "adapters/common.hpp"

namespace arcs::adapters::external_state {

/**
 * @brief Abstract interface for adapters that poll or receive state from an
 *        external system and normalize it into core intake submissions.
 */
class IExternalStateAdapter : public arcs::adapters::IAdapter {
public:
    ~IExternalStateAdapter() override = default;

    /**
     * @brief Polls or receives the latest state signal from the external system.
     * @return The raw state signal.
     */
    virtual nlohmann::json poll_or_receive_state() = 0;

    /**
     * @brief Normalizes a raw external state signal into a draft artifact.
     * @param signal The raw state signal to normalize.
     * @return The normalized draft artifact.
     */
    virtual DraftArtifact normalize_state(const nlohmann::json& signal) = 0;

};

} // namespace arcs::adapters::external_state
