/**
 * @file permission_reducer.hpp
 * @brief Reducer that computes a principal's effective permissions from a
 * stream of permission_grant artifacts.
 */
#pragma once

#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "reducer/effective_permissions.hpp"
#include "reducer/time_source.hpp"

namespace arcs::reducer {

/**
 * @brief Reduces "permission_grant" artifacts belonging to a single
 * principal into their currently active EffectivePermissions, taking each
 * grant's TTL into account relative to the injected time source.
 */
class PermissionReducer {
public:
    /**
     * @brief Constructs a reducer scoped to one principal.
     * @param principal Identifier of the principal to compute permissions for.
     * @param time_source Time source used to evaluate grant TTLs; must
     *        outlive this reducer.
     */
    PermissionReducer(std::string principal, const ITimeSource& time_source);

    /**
     * @brief Computes the effective permissions for the configured
     * principal from the given artifacts, keeping only grants that are
     * addressed to the principal, carry a capability, and are currently
     * within their TTL.
     * @param artifacts Artifact history to reduce over.
     * @return The resulting EffectivePermissions.
     */
    EffectivePermissions reduce(
        const std::vector<arcs::artifact::ArtifactVersion>& artifacts);

private:
    std::string principal_;
    const ITimeSource& time_source_;
};

} // namespace arcs::reducer
