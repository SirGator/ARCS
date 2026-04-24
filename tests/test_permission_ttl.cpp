#include <cassert>
#include <iostream>
#include <vector>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "reducer/permission_reducer.hpp"
#include "reducer/time_source.hpp"

using namespace arcs;

class FixedTimeSource final : public reducer::ITimeSource {
public:
    std::string now() const override {
        return "2026-04-24T12:00:00Z";
    }
};

static artifact::ArtifactVersion make_permission_grant(
    const std::string& principal,
    const std::string& capability,
    const std::string& expires_at
) {
    artifact::ArtifactVersion grant{};
    grant.artifact_id = "a_perm_" + capability;
    grant.version_id = "v_perm_" + capability;
    grant.type = "permission_grant";
    grant.schema_id = "arcs.permission_grant.v1";
    grant.schema_version = 1;

    grant.payload = nlohmann::json{
        {"principal", principal},
        {"capability", capability},
        {"scope", "task_id:t_01"},
        {"expires_at", expires_at}
    };

    return grant;
}

static bool has_capability(
    const reducer::EffectivePermissions& permissions,
    const std::string& capability
) {
    for (const auto& value : permissions.capabilities) {
        if (value == capability) {
            return true;
        }
    }

    return false;
}

static void expired_permission_is_ignored()
{
    FixedTimeSource time_source;

    std::vector<artifact::ArtifactVersion> artifacts{
        make_permission_grant(
            "user:simon",
            "policy:edit",
            "2026-04-24T11:00:00Z"
        )
    };

    reducer::PermissionReducer reducer("user:simon", time_source);
    auto permissions = reducer.reduce(artifacts);

    assert(!has_capability(permissions, "policy:edit"));

    std::cout << "[PASS] expired permission is ignored\n";
}

static void active_permission_is_kept()
{
    FixedTimeSource time_source;

    std::vector<artifact::ArtifactVersion> artifacts{
        make_permission_grant(
            "user:simon",
            "policy:edit",
            "2026-04-24T13:00:00Z"
        )
    };

    reducer::PermissionReducer reducer("user:simon", time_source);
    auto permissions = reducer.reduce(artifacts);

    assert(has_capability(permissions, "policy:edit"));

    std::cout << "[PASS] active permission is kept\n";
}

int main()
{
    expired_permission_is_ignored();
    active_permission_is_kept();

    std::cout << "test_permission_ttl OK\n";
    return 0;
}