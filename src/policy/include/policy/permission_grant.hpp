/**
 * @file permission_grant.hpp
 * @brief Data model and JSON (de)serialization for permission grant artifacts.
 *
 * A permission grant describes a single, scoped, time-bounded capability
 * handed out to a principal (e.g. a user or executor). This file defines
 * the in-memory representation of a grant payload and the free functions
 * used to convert it to/from its JSON artifact form.
 */
#pragma once

#include <string>
#include <optional>

#include <nlohmann/json.hpp>

namespace arcs::artifact {
struct ArtifactVersion;
}

namespace arcs::policy {

using PermissionGrantArtifact = arcs::artifact::ArtifactVersion;

/**
 * @brief Identifies the entity a permission grant is issued to.
 */
struct PrincipalRef {
    std::string id;   // z.B. "user:simon" oder "executor:report"
};

/**
 * @brief Describes the scope a granted capability applies to (e.g. a task,
 * project, namespace, or module).
 */
struct PermissionScope {
    std::string kind;   // z.B. "task" | "project" | "namespace" | "module"
    std::string value;  // z.B. "task_id:t_01H..."
};

/**
 * @brief Time window (time-to-live) during which a permission grant is valid.
 */
struct TTL {
    std::string not_before; // UTC ISO-8601, optional nutzbar
    std::string expires_at; // UTC ISO-8601
};

/**
 * @brief Full payload of a permission grant: who it's for, what capability
 * it authorizes, the scope it is limited to, and its validity window.
 */
struct PermissionGrantPayload {
    PrincipalRef principal;
    std::string capability;   // z.B. "exec:report_emit"
    PermissionScope scope;
    TTL ttl;
};

/**
 * @brief Parses a permission grant payload from its JSON representation.
 * @param j JSON object containing at least "principal" and "capability",
 *          and optionally "scope" and "expires_at".
 * @return The parsed PermissionGrantPayload.
 */
PermissionGrantPayload permission_grant_from_json(const nlohmann::json& j);

/**
 * @brief Serializes a permission grant payload to its JSON representation.
 * @param grant The permission grant payload to serialize.
 * @return JSON object representing the grant.
 */
nlohmann::json permission_grant_to_json(const PermissionGrantPayload& grant);

} // namespace arcs::policy
