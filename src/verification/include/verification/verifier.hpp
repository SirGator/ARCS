#pragma once

#include <memory>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "schema/validation_result.hpp"

namespace arcs {

using arcs::artifact::ActorRef;
using arcs::artifact::ArtifactVersion;
using arcs::artifact::SourceRef;
using arcs::artifact::TrustInfo;

namespace schema {
class SchemaRegistry;
} // namespace schema

namespace store {
class IStore;
} // namespace store

namespace reducer {
class ITimeSource;
} // namespace reducer

// -----------------------------
// Basis-Enums
// -----------------------------

enum class CheckStatus {
    Pass,
    Fail,
    Unknown
};

// Hilfsfunktionen für Logs / JSON / Debugging
std::string to_string(CheckStatus status);
CheckStatus check_status_from_string(const std::string& value);

// -----------------------------
// Referenzen
// -----------------------------

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

struct EffectivePermissions {
    std::vector<std::string> capabilities;
    std::vector<std::string> scopes;

    bool has_capability(const std::string& capability) const;
    bool has_scope(const std::string& scope) const;

    bool operator==(const EffectivePermissions& other) const
    {
        return capabilities == other.capabilities && scopes == other.scopes;
    }
};

// -----------------------------
// Verification Report Payload
// -----------------------------

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

struct VerificationContext {
    const ArtifactVersion* policy{nullptr};
    EffectivePermissions permissions{};

    const schema::SchemaRegistry* schema_registry{nullptr};
    const store::IStore* store{nullptr};
    const reducer::ITimeSource* time_source{nullptr};

    // Actor, für Permission/Authority-Checks später nützlich
    std::optional<ActorRef> principal{};
};

// -----------------------------
// Verifier Interface
// -----------------------------

class IVerifier {
public:
    virtual ~IVerifier() = default;

    virtual VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const = 0;
};

// -----------------------------
// Core Verifier
// -----------------------------

class SchemaVerifier final : public IVerifier {
public:
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

class ReferenceIntegrityVerifier final : public IVerifier {
public:
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

class PermissionVerifier final : public IVerifier {
public:
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

class ApprovalVerifier final : public IVerifier {
public:
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

class ScopeVerifier final : public IVerifier {
public:
    VerificationCheck check(
        const ArtifactVersion& target,
        const VerificationContext& context) const override;
};

// -----------------------------
// Verification Engine
// -----------------------------

class VerificationEngine {
public:
    void add_verifier(std::shared_ptr<IVerifier> verifier);

    VerificationReportData run_all(
        const ArtifactVersion& target,
        const VerificationContext& context) const;

private:
    std::vector<std::shared_ptr<IVerifier>> verifiers_;
};

// -----------------------------
// Hilfsfunktionen
// -----------------------------

CheckStatus aggregate_status(const std::vector<VerificationCheck>& checks);

VerificationReportData make_verification_report(
    const ArtifactVersion& target,
    std::vector<VerificationCheck> checks);

// Optional: direkt ARCS-Artefakt daraus bauen
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

void to_json(nlohmann::json& j, const ArtifactRef& ref);
void from_json(const nlohmann::json& j, ArtifactRef& ref);

void to_json(nlohmann::json& j, const VerificationCheck& check);
void from_json(const nlohmann::json& j, VerificationCheck& check);

void to_json(nlohmann::json& j, const VerificationReportData& report);
void from_json(const nlohmann::json& j, VerificationReportData& report);

} // namespace arcs
