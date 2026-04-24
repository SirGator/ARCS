#include "policy/permission_grant.hpp"

#include <stdexcept>
#include <string>

namespace arcs::policy {

namespace {

void require_not_empty(const std::string& value, const char* field) {
    if (value.empty()) {
        throw std::invalid_argument(std::string(field) + " must not be empty");
    }
}

} // namespace

PermissionGrantPayload permission_grant_from_json(const nlohmann::json& j) {
    PermissionGrantPayload grant{};

    grant.principal.id = j.at("principal").get<std::string>();
    grant.capability   = j.at("capability").get<std::string>();

    require_not_empty(grant.principal.id, "principal");
    require_not_empty(grant.capability, "capability");

    if (j.contains("scope")) {
        grant.scope.kind = "raw";
        grant.scope.value = j.at("scope").get<std::string>();
    }

    if (j.contains("expires_at")) {
        grant.ttl.expires_at = j.at("expires_at").get<std::string>();
    }

    return grant;
}

nlohmann::json permission_grant_to_json(const PermissionGrantPayload& grant) {
    nlohmann::json j;

    j["principal"] = grant.principal.id;
    j["capability"] = grant.capability;

    if (!grant.scope.value.empty()) {
        j["scope"] = grant.scope.value;
    }

    if (!grant.ttl.expires_at.empty()) {
        j["expires_at"] = grant.ttl.expires_at;
    }

    return j;
}

} // namespace arcs::policy