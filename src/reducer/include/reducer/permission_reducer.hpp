#pragma once

#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "reducer/effective_permissions.hpp"
#include "reducer/time_source.hpp"

namespace arcs::reducer {

class PermissionReducer {
public:
    PermissionReducer(std::string principal, const ITimeSource& time_source);

    EffectivePermissions reduce(
        const std::vector<arcs::artifact::ArtifactVersion>& artifacts);

private:
    std::string principal_;
    const ITimeSource& time_source_;
};

} // namespace arcs::reducer
