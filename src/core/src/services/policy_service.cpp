/**
 * @file policy_service.cpp
 * @brief Implements the policy pipeline stage: bootstrap/store-backed policy
 *        repositories and PolicyService, which exposes the current and
 *        previous policy versions for a given scope by delegating to an IPolicyRepository.
 */
#include "core/services/policy_service.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>

#include "artifact/factory.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief Formats the current UTC time as an ISO-8601 timestamp string ("%Y-%m-%dT%H:%M:%SZ").
 * @return The current UTC timestamp string.
 */
std::string utc_now()
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());
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

} // namespace

/**
 * @brief Builds a hard-coded demo policy snapshot with a "current" version (granting the
 *        exec:report_emit capability, hard verifier checks for permission/scope/policy_drift,
 *        and an approval requirement on exec:report_emit) and a "previous" version derived
 *        from it. The scope parameter is currently unused.
 * @param scope Scope requested (unused in the demo implementation).
 * @return The demo PolicySnapshot containing current and previous policy artifacts.
 */
PolicySnapshot make_bootstrap_policy_snapshot(const std::string& scope)
{
    (void)scope;

    const auto policy_base = make_artifact(
        "policy",
        "arcs.policy.v1",
        "policy:core",
        "system",
        "kernel",
        "internal",
        "policy_bootstrap",
        "high",
        "system");

    PolicySnapshot snapshot;
    snapshot.current = policy_base;
    snapshot.current.artifact_id = "a_policy_core";
    snapshot.current.version_id = "v_policy_002";
    snapshot.current.payload = nlohmann::json{
        {"capabilities", {"exec:report_emit"}},
        {"constraints", nlohmann::json::object()},
        {"verifier_rules", {
            {"hard_checks", {"permission", "scope", "policy_drift"}},
            {"soft_checks", nlohmann::json::array()},
        }},
        {"approval_required_for", {"exec:report_emit"}},
    };
    snapshot.current.provenance.rules_applied = {"policy_bootstrap"};
    snapshot.current.provenance.transform = "policy_current";

    snapshot.previous = snapshot.current;
    snapshot.previous.version_id = "v_policy_001";
    snapshot.previous.payload["verifier_rules"]["hard_checks"] = {"permission", "scope", "policy_drift"};
    snapshot.previous.provenance.transform = "policy_previous";
    return snapshot;
}

PolicySnapshot BootstrapPolicyRepository::load_policy_snapshot(const std::string& scope) const
{
    return make_bootstrap_policy_snapshot(scope);
}

StorePolicyRepository::StorePolicyRepository(const arcs::store::IStore& store)
    : store_(store)
{
}

PolicySnapshot StorePolicyRepository::load_policy_snapshot(const std::string& scope) const
{
    (void)scope;

    if (!store_.has_artifact("a_policy_core")) {
        return {};
    }

    PolicySnapshot snapshot;
    snapshot.current = store_.get("a_policy_core");

    if (store_.has_version("v_policy_001")) {
        snapshot.previous = store_.get_version("v_policy_001");
    } else {
        snapshot.previous = snapshot.current;
    }

    return snapshot;
}

FallbackPolicyRepository::FallbackPolicyRepository(
    std::unique_ptr<IPolicyRepository> primary,
    std::unique_ptr<IPolicyRepository> fallback)
    : primary_(std::move(primary))
    , fallback_(std::move(fallback))
{
}

PolicySnapshot FallbackPolicyRepository::load_policy_snapshot(const std::string& scope) const
{
    const auto snapshot = primary_->load_policy_snapshot(scope);
    if (!snapshot.current.artifact_id.empty()) {
        return snapshot;
    }

    return fallback_->load_policy_snapshot(scope);
}

PolicyService::PolicyService(const IPolicyRepository& repository)
    : repository_(repository)
{
}

/**
 * @brief Loads the policy snapshot (current and previous versions) for a scope.
 * @param scope Scope to load the policy snapshot for.
 * @return The PolicySnapshot returned by the underlying repository.
 */
PolicySnapshot PolicyService::load_policy_snapshot(const std::string& scope) const
{
    return repository_.load_policy_snapshot(scope);
}

/**
 * @brief Retrieves the currently active policy artifact for a scope.
 * @param scope Scope to retrieve the current policy for.
 * @return The current policy artifact version.
 */
arcs::artifact::ArtifactVersion PolicyService::current_policy_for_scope(const std::string& scope) const
{
    return load_policy_snapshot(scope).current;
}

/**
 * @brief Retrieves the previous policy artifact for a scope, if one exists.
 * @param scope Scope to retrieve the previous policy for.
 * @return The previous policy artifact version, or std::nullopt if none exists.
 */
std::optional<arcs::artifact::ArtifactVersion> PolicyService::previous_policy_for_scope(const std::string& scope) const
{
    return load_policy_snapshot(scope).previous;
}

} // namespace arcs::core::services
