#pragma once

#include <string>
#include <optional>

#include <nlohmann/json.hpp>

namespace arcs::artifact {
struct ArtifactVersion;
}

namespace arcs::policy {

using PermissionGrantArtifact = arcs::artifact::ArtifactVersion;

struct PrincipalRef {
    std::string id;   // z.B. "user:simon" oder "executor:report"
};

struct PermissionScope {
    std::string kind;   // z.B. "task" | "project" | "namespace" | "module"
    std::string value;  // z.B. "task_id:t_01H..."
};

struct TTL {
    std::string not_before; // UTC ISO-8601, optional nutzbar
    std::string expires_at; // UTC ISO-8601
};

struct PermissionGrantPayload {
    PrincipalRef principal;
    std::string capability;   // z.B. "exec:report_emit"
    PermissionScope scope;
    TTL ttl;
};

PermissionGrantPayload permission_grant_from_json(const nlohmann::json& j);
nlohmann::json permission_grant_to_json(const PermissionGrantPayload& grant);

} // namespace arcs::policy
