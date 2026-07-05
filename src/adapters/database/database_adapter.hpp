/**
 * @file database_adapter.hpp
 * @brief Defines the interface that database-backed adapters must implement
 *        to plug into ARCS's governance pipeline (capability checks, reads,
 *        verified writes, and result submission back into the core).
 */

#pragma once

#include <nlohmann/json.hpp>

#include "adapters/common.hpp"

namespace arcs::adapters::database {

/**
 * @brief Abstract interface for adapters that read from and write to an
 *        external database on behalf of ARCS.
 *
 * Implementations are expected to gate every read/write with a capability
 * check, translate between the adapter-native representation and the
 * ARCS intake model, and report the outcome of writes back to the caller.
 */
class IDatabaseAdapter : public arcs::adapters::IAdapter {
public:
    ~IDatabaseAdapter() override = default;

    /**
     * @brief Checks whether the given request is permitted to perform a read.
     * @param request The raw read request to evaluate.
     * @return The capability check outcome, including diagnostics.
     */
    virtual CapabilityCheckResult can_read(const nlohmann::json& request) const = 0;

    /**
     * @brief Executes a read against the underlying database.
     * @param request The raw read request describing what to fetch.
     * @return The resulting data represented as a draft artifact.
     */
    virtual DraftArtifact read(const nlohmann::json& request) = 0;

    /**
     * @brief Checks whether the given draft artifact is permitted to be written.
     * @param action The draft artifact representing the proposed write.
     * @return The capability check outcome, including diagnostics.
     */
    virtual CapabilityCheckResult can_write(const DraftArtifact& action) const = 0;

    /**
     * @brief Performs a write that has already passed capability verification.
     * @param action The draft artifact representing the write to execute.
     * @return The raw response from the underlying database.
     */
    virtual nlohmann::json execute_verified_write(const DraftArtifact& action) = 0;

    /**
     * @brief Converts a raw database response into the ARCS artifact model.
     * @param response The raw response returned by the database.
     * @return The response represented as a draft artifact.
     */
    virtual DraftArtifact convert_response(const nlohmann::json& response) const = 0;

};

} // namespace arcs::adapters::database
