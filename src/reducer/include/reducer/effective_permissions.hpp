/**
 * @file effective_permissions.hpp
 * @brief Defines the reduced set of capabilities and scopes currently
 * granted to a principal.
 */
#pragma once

#include <string>
#include <vector>

namespace arcs::reducer {

/**
 * @brief The set of capabilities and scopes currently in effect for a
 * given principal, as computed by PermissionReducer from active grants.
 */
struct EffectivePermissions {
    std::string principal;
    std::vector<std::string> capabilities;
    std::vector<std::string> scopes;

    /** @brief Returns true if the given capability is currently granted. */
    bool has_capability(const std::string& capability) const;
    /** @brief Returns true if the given scope is currently granted. */
    bool has_scope(const std::string& scope) const;

    bool operator==(const EffectivePermissions& other) const
    {
        return principal == other.principal &&
               capabilities == other.capabilities &&
               scopes == other.scopes;
    }
};

} // namespace arcs::reducer
