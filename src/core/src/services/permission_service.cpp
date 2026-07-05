/**
 * @file permission_service.cpp
 * @brief Implements PermissionService, the pipeline stage that resolves the effective
 *        permission set for a principal/scope pair.
 *
 * It optionally synthesizes a demo permission-grant artifact, feeds any accumulated
 * grant artifacts through the PermissionReducer, and ensures the requested scope is
 * present in the resulting EffectivePermissions.
 */
#include "core/services/permission_service.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>

#include <nlohmann/json.hpp>

#include "artifact/factory.hpp"
#include "reducer/permission_reducer.hpp"
#include "store/store.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief Formats a time point as a UTC ISO-8601 timestamp string ("%Y-%m-%dT%H:%M:%SZ").
 * @param time_point The time point to format.
 * @return The UTC timestamp string.
 */
std::string utc_at(const std::chrono::system_clock::time_point time_point)
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(time_point);
    std::tm tm{};

#if defined(_WIN32)
    gmtime_s(&tm, &now_time_t);
#else
    gmtime_r(&now_time_t, &tm);
#endif

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
    return out.str();
}

/**
 * @brief Formats the current UTC time as an ISO-8601 timestamp string.
 * @return The current UTC timestamp string.
 */
std::string utc_now()
{
    return utc_at(std::chrono::system_clock::now());
}

/**
 * @brief Formats the UTC time a given number of hours in the future.
 * @param hours Number of hours to add to the current time.
 * @return The resulting UTC timestamp string.
 */
std::string utc_after_hours(int hours)
{
    return utc_at(std::chrono::system_clock::now() + std::chrono::hours(hours));
}

/**
 * @brief Builds a base artifact version populated with the given identity, provenance,
 *        and trust metadata, timestamped at the current UTC time.
 * @param type Artifact type name.
 * @param schema_id Schema identifier the artifact conforms to.
 * @param stream_key Stream key the artifact belongs to.
 * @param actor_type Type of the actor producing the artifact.
 * @param actor_id Identifier of the actor producing the artifact.
 * @param source_kind Kind of source that originated the artifact.
 * @param source_ref Reference to the originating source.
 * @param trust_level Trust level assigned to the artifact.
 * @param trust_source_class Trust source classification.
 * @return The newly constructed base ArtifactVersion.
 */
arcs::artifact::ArtifactVersion make_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& actor_type,
    const std::string& actor_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class)
{
    return arcs::artifact::factory::make_base_artifact(
        type,
        schema_id,
        stream_key,
        actor_type,
        actor_id,
        source_kind,
        source_ref,
        trust_level,
        trust_source_class,
        utc_now());
}

/**
 * @brief Creates a demo permission-grant artifact granting the "exec:report_emit"
 *        capability to a principal within a scope, expiring one hour from now.
 * @param principal Identifier of the principal receiving the grant.
 * @param scope Scope (stream key) the grant applies to.
 * @return The permission_grant artifact version.
 */
arcs::artifact::ArtifactVersion make_demo_permission_grant(
    const std::string& principal,
    const std::string& scope)
{
    auto grant = make_artifact(
        "permission_grant",
        "arcs.permission_grant.v1",
        scope,
        "system",
        "permission_service",
        "internal",
        "demo_permission",
        "high",
        "system");
    grant.payload = nlohmann::json{
        {"principal", principal},
        {"capability", "exec:report_emit"},
        {"scope", scope},
        {"expires_at", utc_after_hours(1)},
    };
    grant.provenance.rules_applied = {"demo_permission_grant"};
    grant.provenance.transform = "grant_permission";
    return grant;
}

} // namespace

/**
 * @brief Resolves the effective permissions for a principal in a given scope, optionally
 *        injecting a synthetic demo permission grant before reducing all grant artifacts.
 * @param principal Identifier of the principal whose permissions are being resolved.
 * @param scope Scope to resolve/ensure permissions for; if non-empty and not already
 *        present, it is added to the resulting scopes list.
 * @param time_source Time source used by the reducer to evaluate grant validity/expiry.
 * @param demo_permission_granted Whether to synthesize and include a demo permission
 *        grant artifact for the principal/scope.
 * @return A PermissionResolution containing the effective permissions and the artifacts
 *         used to derive them.
 */
StorePermissionSource::StorePermissionSource(const arcs::store::IStore& store)
    : store_(store)
{
}

std::vector<arcs::artifact::ArtifactVersion> StorePermissionSource::load_permission_grants(
    const std::string& principal,
    const std::string& scope) const
{
    std::vector<arcs::artifact::ArtifactVersion> grants;
    for (const auto& artifact : store_.list(arcs::store::ListQuery{.type = std::string{"permission_grant"}})) {
        if (!artifact.payload.is_object()) {
            continue;
        }

        if (artifact.payload.value("principal", std::string{}) != principal) {
            continue;
        }

        const auto artifact_scope = artifact.payload.value("scope", std::string{});
        if (!scope.empty() && !artifact_scope.empty() && artifact_scope != scope) {
            continue;
        }

        grants.push_back(artifact);
    }

    return grants;
}

std::vector<arcs::artifact::ArtifactVersion> DemoPermissionSource::load_permission_grants(
    const std::string& principal,
    const std::string& scope) const
{
    return {make_demo_permission_grant(principal, scope)};
}

CompositePermissionSource::CompositePermissionSource(
    const IPermissionSource& first,
    const IPermissionSource& second)
    : first_(first)
    , second_(second)
{
}

std::vector<arcs::artifact::ArtifactVersion> CompositePermissionSource::load_permission_grants(
    const std::string& principal,
    const std::string& scope) const
{
    auto grants = first_.load_permission_grants(principal, scope);
    auto extra = second_.load_permission_grants(principal, scope);
    grants.insert(grants.end(), extra.begin(), extra.end());
    return grants;
}

PermissionResolution PermissionService::resolve_permissions(
    const std::string& principal,
    const std::string& scope,
    const arcs::reducer::ITimeSource& time_source,
    const IPermissionSource& source) const
{
    PermissionResolution resolution{};
    resolution.artifacts = source.load_permission_grants(principal, scope);

    arcs::reducer::PermissionReducer reducer(principal, time_source);
    resolution.permissions = reducer.reduce(resolution.artifacts);
    if (!scope.empty() && !resolution.permissions.has_scope(scope)) {
        resolution.permissions.scopes.push_back(scope);
    }
    return resolution;
}

} // namespace arcs::core::services
