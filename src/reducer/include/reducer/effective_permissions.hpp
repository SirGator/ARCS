#pragma once

#include <string>
#include <vector>

namespace arcs::reducer {

struct EffectivePermissions {
    std::string principal;
    std::vector<std::string> capabilities;
    std::vector<std::string> scopes;

    bool has_capability(const std::string& capability) const;
    bool has_scope(const std::string& scope) const;

    bool operator==(const EffectivePermissions& other) const
    {
        return principal == other.principal &&
               capabilities == other.capabilities &&
               scopes == other.scopes;
    }
};

} // namespace arcs::reducer
