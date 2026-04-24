#include "reducer/permission_reducer.hpp"

#include "policy/permission_grant.hpp"

#include <algorithm>
#include <string>
#include <utility>

namespace arcs::reducer {

namespace {

bool contains_string(const std::vector<std::string>& values, const std::string& value)
{
    return std::find(values.begin(), values.end(), value) != values.end();
}

bool is_active_ttl(const arcs::policy::TTL& ttl, const std::string& now)
{
    if (!ttl.not_before.empty() && now < ttl.not_before) {
        return false;
    }

    if (!ttl.expires_at.empty() && now >= ttl.expires_at) {
        return false;
    }

    return true;
}

arcs::policy::PermissionGrantPayload parse_permission_grant(
    const arcs::artifact::ArtifactVersion& artifact
) {
    arcs::policy::PermissionGrantPayload grant{};

    const auto& p = artifact.payload;

    if (p.contains("principal") && p["principal"].is_string()) {
        grant.principal.id = p["principal"].get<std::string>();
    }

    if (p.contains("capability") && p["capability"].is_string()) {
        grant.capability = p["capability"].get<std::string>();
    }

    if (p.contains("scope")) {
        if (p["scope"].is_string()) {
            grant.scope.kind = "raw";
            grant.scope.value = p["scope"].get<std::string>();
        } else if (p["scope"].is_object()) {
            const auto& s = p["scope"];

            if (s.contains("kind") && s["kind"].is_string()) {
                grant.scope.kind = s["kind"].get<std::string>();
            }

            if (s.contains("value") && s["value"].is_string()) {
                grant.scope.value = s["value"].get<std::string>();
            }
        }
    }

    if (p.contains("not_before") && p["not_before"].is_string()) {
        grant.ttl.not_before = p["not_before"].get<std::string>();
    }

    if (p.contains("expires_at") && p["expires_at"].is_string()) {
        grant.ttl.expires_at = p["expires_at"].get<std::string>();
    }

    return grant;
}

} // namespace

PermissionReducer::PermissionReducer(std::string principal, const ITimeSource& time_source)
    : principal_(std::move(principal)),
      time_source_(time_source)
{
}

EffectivePermissions PermissionReducer::reduce(
    const std::vector<arcs::artifact::ArtifactVersion>& artifacts
) {
    EffectivePermissions result{};
    result.principal = principal_;

    const auto now = time_source_.now();

    for (const auto& artifact : artifacts) {
        if (artifact.type != "permission_grant") {
            continue;
        }

        const auto grant = parse_permission_grant(artifact);

        if (grant.principal.id != principal_) {
            continue;
        }

        if (grant.capability.empty()) {
            continue;
        }

        if (!is_active_ttl(grant.ttl, now)) {
            continue;
        }

        if (!contains_string(result.capabilities, grant.capability)) {
            result.capabilities.push_back(grant.capability);
        }

        if (!grant.scope.value.empty()) {
            result.scopes.push_back(grant.scope.value);
        }
    }

    return result;
}

} // namespace arcs::reducer
