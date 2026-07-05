/**
 * @file verifier.hpp
 * @brief Core verification framework for ARCS: defines the IVerifier
 *        interface, the concrete built-in verifiers (schema, reference
 *        integrity, permission, approval, scope, policy head), the
 *        VerificationEngine that runs a set of verifiers over an artifact
 *        version, the VerifierRegistry used to build verification plans by
 *        name, and the report/JSON types used to represent verification
 *        results.
 */
#pragma once

#include <memory>
#include <functional>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "reducer/effective_permissions.hpp"
#include "schema/validation_result.hpp"

namespace arcs::schema {
class SchemaRegistry;
}

namespace arcs::store {
class IStore;
}

namespace arcs::reducer {
class ITimeSource;
}

namespace arcs::verification {

using arcs::artifact::ActorRef;
using arcs::artifact::ArtifactVersion;
using arcs::artifact::SourceRef;
using arcs::artifact::TrustInfo;

// -----------------------------
// Basis-Enums
// -----------------------------

/**
 * @brief Outcome of a single verification check.
 */
enum class CheckStatus {
    Pass,    /**< The check succeeded. */
    Fail,    /**< The check failed; the target is not verified. */
    Unknown  /**< The check could not be decided deterministically. */
};

/**
 * @brief Converts a CheckStatus to its lowercase string representation
 *        (e.g. for logging or JSON serialization).
 * @param status Status value to convert.
 * @return String form: "pass", "fail", or "unknown".
 */
std::string to_string(CheckStatus status);

/**
 * @brief Parses a CheckStatus from its lowercase string representation.
 * @param value String form: "pass", "fail", or "unknown".
 * @return The corresponding CheckStatus.
 */
CheckStatus check_status_from_string(const std::string& value);

// -----------------------------
// Referenzen
// -----------------------------

/**
 * @brief Identifies a specific version of an artifact by artifact and
 *        version id, used to reference other artifacts from within a
 *        verification target's payload.
 */
struct ArtifactRef {
    std::string artifact_id;
    std::string version_id;

    bool operator==(const ArtifactRef& other) const
    {
        return artifact_id == other.artifact_id && version_id == other.version_id;
    }
};

// -----------------------------
// Einzelne Prüfergebnisse
// -----------------------------

/**
 * @brief Result of a single named verification check (e.g. "schema",
 *        "permission", "approval"), including its status and a
 *        human-readable detail message.
 */
struct VerificationCheck {
    std::string name;
    CheckStatus status{CheckStatus::Unknown};
    std::string detail;

    bool operator==(const VerificationCheck& other) const
    {
        return name == other.name && status == other.status && detail == other.detail;
    }
};

// -----------------------------
// Effektive Permissions
// -----------------------------

using EffectivePermissions = arcs::reducer::EffectivePermissions;

// -----------------------------
// Verification Report Payload
// -----------------------------

/**
 * @brief Aggregated verification result for an artifact version: the
 *        overall status derived from all individual checks, the full list
 *        of checks that were run, and any blockers or recommendations
 *        surfaced to the caller.
 */
struct VerificationReportData {
    ArtifactRef target;
    CheckStatus status{CheckStatus::Unknown};
    std::vector<VerificationCheck> checks;
    std::vector<std::string> blockers;
    std::vector<std::string> recommendations;

    bool operator==(const VerificationReportData& other) const
    {
        return target == other.target && status == other.status &&
               checks == other.checks && blockers == other.blockers &&
               recommendations == other.recommendations;
    }
};

// -----------------------------
// Verification Context
// -----------------------------

/**
 * @brief Bundles everything a verifier needs to evaluate a target: the
 *        current policy head, the effective permissions of the acting
 *        principal, and read-only access to the schema registry, store,
 *        and time source. Individual verifiers treat missing optional
 *        members (nullptr) as reasons to report CheckStatus::Unknown
 *        rather than guessing.
 */
struct VerificationContext {
    const ArtifactVersion* policy{nullptr};
    EffectivePermissions permissions{};

    const arcs::schema::SchemaRegistry* schema_registry{nullptr};
    const arcs::store::IStore* store{nullptr};
    const arcs::reducer::ITimeSource* time_source{nullptr};

    // Actor, für Permission/Authority-Checks später nützlich
    std::optional<ActorRef> principal{};
};

// -----------------------------
// Verifier Interface
// -----------------------------

/**
 * @brief Base interface implemented by all verifiers.
 *
 * A verifier evaluates a single concern (schema validity, reference
 * integrity, permissions, approvals, scope, or policy drift) against an
 * artifact version and returns one VerificationCheck describing the
 * outcome.
 */
class IVerifier {
public:
    virtual ~IVerifier() = default;

    /**
     * @brief Evaluates this verifier's concern against the given target.
     * @param target Artifact version being verified.
     * @param context Verification context (policy, permissions, store, etc.).
     * @return The resulting VerificationCheck.
     */
    virtual VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const = 0;
};

// -----------------------------
// Core Verifier
// -----------------------------

/**
 * @brief Verifies that the target's payload conforms to its declared
 *        JSON schema, using the schema registry supplied in the context.
 */
class SchemaVerifier final : public IVerifier {
public:
    /**
     * @brief Validates the target payload against its schema.
     * @param target Artifact version being verified.
     * @param context Verification context; must provide a schema registry.
     * @return VerificationCheck named "schema" indicating pass/fail/unknown.
     */
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

/**
 * @brief Verifies that every artifact/version reference embedded in the
 *        target's payload resolves to an existing, matching version in
 *        the store.
 */
class ReferenceIntegrityVerifier final : public IVerifier {
public:
    /**
     * @brief Recursively scans the target payload for artifact references
     *        and confirms each one resolves correctly in the store.
     * @param target Artifact version being verified.
     * @param context Verification context; must provide a store.
     * @return VerificationCheck named "reference_integrity".
     */
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

/**
 * @brief Verifies that the acting principal holds all capabilities
 *        declared as required by the target's payload.
 */
class PermissionVerifier final : public IVerifier {
public:
    /**
     * @brief Checks the target's declared required permissions against
     *        the effective permissions in the context.
     * @param target Artifact version being verified.
     * @param context Verification context, including effective permissions.
     * @return VerificationCheck named "permission".
     */
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

/**
 * @brief Verifies that "action" targets are bound to a matching, valid,
 *        unexpired approval artifact recorded in the store.
 */
class ApprovalVerifier final : public IVerifier {
public:
    /**
     * @brief Locates and validates the approval bound to an action target,
     *        checking option/policy/candidate refs, scope, expiry, and
     *        policy/store head consistency.
     * @param target Artifact version being verified.
     * @param context Verification context; must provide a policy and store.
     * @return VerificationCheck named "approval".
     */
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

/**
 * @brief Verifies that the target's required scopes, if any, match its
 *        stream and fall within the principal's allowed permission scopes.
 */
class ScopeVerifier final : public IVerifier {
public:
    /**
     * @brief Checks required scopes declared in the target payload against
     *        the stream-derived scope and the context's allowed scopes.
     * @param target Artifact version being verified.
     * @param context Verification context, including effective permissions.
     * @return VerificationCheck named "scope".
     */
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

/**
 * @brief Verifies that a target's embedded policy reference still points
 *        at the current policy head, guarding against decisions made
 *        against a stale policy.
 */
class PolicyHeadVerifier final : public IVerifier {
public:
    /**
     * @brief Compares the target's policy_ref against the current policy
     *        head in the context.
     * @param target Artifact version being verified.
     * @param context Verification context; must provide the current policy.
     * @return VerificationCheck named "policy_drift".
     */
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

/**
 * @brief A resolved set of verifiers to run, plus the names of any
 *        requested checks that had no registered factory.
 */
struct VerificationPlan {
    std::vector<std::shared_ptr<IVerifier>> verifiers;
    std::vector<std::string> unsupported_checks;
};

/**
 * @brief Maps named checks (e.g. "schema", "permission") to factories that
 *        construct the corresponding IVerifier, and builds VerificationPlan
 *        instances from a requested list of check names.
 */
class VerifierRegistry {
public:
    using VerifierFactory = std::function<std::shared_ptr<IVerifier>()>;

    /**
     * @brief Registers a factory function for a named check.
     * @param check_name Unique name of the check (e.g. "schema").
     * @param factory Factory that constructs the verifier instance.
     */
    void register_factory(const std::string& check_name, VerifierFactory factory);

    /**
     * @brief Builds a verification plan by resolving each requested check
     *        name to a verifier instance via its registered factory.
     * @param check_names Names of the checks to include in the plan.
     * @return A VerificationPlan containing the constructed verifiers and
     *         the names of any checks with no registered factory.
     */
    VerificationPlan build_plan(const std::vector<std::string>& check_names) const;

    /**
     * @brief Builds a registry pre-populated with the built-in core
     *        verifiers ("permission", "scope", "approval", "policy_drift").
     * @return A VerifierRegistry with the core verifiers registered.
     */
    static VerifierRegistry with_core_verifiers();

private:
    std::unordered_map<std::string, VerifierFactory> factories_;
};

// -----------------------------
// Verification Engine
// -----------------------------

/**
 * @brief Runs a fixed collection of verifiers over an artifact version and
 *        aggregates their individual checks into a single
 *        VerificationReportData.
 */
class VerificationEngine {
public:
    /**
     * @brief Adds a verifier to the engine's collection.
     * @param verifier Verifier instance to add; must not be null.
     */
    void add_verifier(std::shared_ptr<IVerifier> verifier);

    /**
     * @brief Runs every registered verifier against the target and
     *        aggregates the results.
     * @param target Artifact version being verified.
     * @param context Verification context shared by all verifiers.
     * @return The aggregated VerificationReportData.
     */
    VerificationReportData run_all(
        const ArtifactVersion& target,
        const VerificationContext& context) const;

private:
    std::vector<std::shared_ptr<IVerifier>> verifiers_;
};

// -----------------------------
// Hilfsfunktionen
// -----------------------------

/**
 * @brief Derives an overall status from a set of individual checks: Fail
 *        wins over Unknown, which wins over Pass.
 * @param checks Individual verification checks to aggregate.
 * @return The aggregated CheckStatus.
 */
CheckStatus aggregate_status(const std::vector<VerificationCheck>& checks);

/**
 * @brief Builds a VerificationReportData for a target from a list of
 *        individual checks, computing the aggregate status and populating
 *        blockers for any failing or unknown checks.
 * @param target Artifact version the checks were run against.
 * @param checks Individual verification checks to include in the report.
 * @return The assembled VerificationReportData.
 */
VerificationReportData make_verification_report(
    const ArtifactVersion& target,
    std::vector<VerificationCheck> checks);

// Optional: direkt ARCS-Artefakt daraus bauen
/**
 * @brief Wraps a VerificationReportData as a standalone
 *        "verification_report" ArtifactVersion, with provenance linking it
 *        back to the verified target.
 * @param target Artifact version that was verified.
 * @param report Verification report payload to embed.
 * @param created_by Actor that produced the report.
 * @param source Source reference describing where the report originated.
 * @param trust Trust metadata to attach to the new artifact.
 * @param artifact_id Identifier to assign to the new artifact.
 * @param version_id Version identifier to assign to the new artifact.
 * @param stream_key Stream key to assign to the new artifact.
 * @param created_at Creation timestamp to assign to the new artifact.
 * @return The constructed verification report ArtifactVersion.
 */
ArtifactVersion make_verification_report_artifact(
    const ArtifactVersion& target,
    const VerificationReportData& report,
    const ActorRef& created_by,
    const SourceRef& source,
    const TrustInfo& trust,
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& stream_key,
    const std::string& created_at);

// -----------------------------
// JSON
// -----------------------------

/** @brief Serializes an ArtifactRef to JSON. */
void to_json(nlohmann::json& j, const ArtifactRef& ref);
/** @brief Deserializes an ArtifactRef from JSON. */
void from_json(const nlohmann::json& j, ArtifactRef& ref);

/** @brief Serializes a VerificationCheck to JSON. */
void to_json(nlohmann::json& j, const VerificationCheck& check);
/** @brief Deserializes a VerificationCheck from JSON. */
void from_json(const nlohmann::json& j, VerificationCheck& check);

/** @brief Serializes a VerificationReportData to JSON. */
void to_json(nlohmann::json& j, const VerificationReportData& report);
/** @brief Deserializes a VerificationReportData from JSON. */
void from_json(const nlohmann::json& j, VerificationReportData& report);

} // namespace arcs::verification
