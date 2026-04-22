#include <memory>
#include <stdexcept>
#include <vector>

#include <gtest/gtest.h>

#include "artifact/artifact.hpp"
#include "verification/verifier.hpp"

using arcs::artifact::ActorRef;
using arcs::artifact::ArtifactVersion;
using arcs::artifact::SourceRef;
using arcs::artifact::TrustInfo;
using namespace arcs::verification;

namespace {

class StubVerifier final : public IVerifier {
public:
    explicit StubVerifier(VerificationCheck check)
        : check_(std::move(check)) {}

    VerificationCheck check(
        const ArtifactVersion&,
        const VerificationContext&) const override {
        return check_;
    }

private:
    VerificationCheck check_;
};

ArtifactVersion make_target() {
    ArtifactVersion target{};
    target.artifact_id = "a_option_01";
    target.version_id = "v_option_01";
    target.version = 1;
    target.type = "option";
    target.schema_id = "arcs.option.v1";
    target.schema_version = 1;
    target.created_at = "2026-04-20T12:00:00Z";
    target.created_by = ActorRef{
        .actor_type = "human",
        .id = "user:simon",
    };
    target.source = SourceRef{
        .kind = "internal",
        .ref = "test",
    };
    target.trust = TrustInfo{
        .level = "high",
        .source_class = "human",
    };
    target.stream_key = "task_id:t_01";
    target.payload = {
        {"title", "Generate report"},
        {"requires_permissions", {"exec:report_emit"}},
        {"required_scopes", {"task_id:t_01"}}
    };
    return target;
}

} // namespace

TEST(VerificationEngineTest, AggregateStatusReturnsPassWhenAllChecksPass) {
    const std::vector<VerificationCheck> checks = {
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "ok"},
        VerificationCheck{.name = "permission", .status = CheckStatus::Pass, .detail = "ok"},
        VerificationCheck{.name = "scope", .status = CheckStatus::Pass, .detail = "ok"},
    };

    EXPECT_EQ(aggregate_status(checks), CheckStatus::Pass);
}

TEST(VerificationEngineTest, AggregateStatusReturnsFailWhenAnyCheckFails) {
    const std::vector<VerificationCheck> checks = {
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "ok"},
        VerificationCheck{.name = "permission", .status = CheckStatus::Fail, .detail = "capability exec:report_emit fehlt"},
        VerificationCheck{.name = "scope", .status = CheckStatus::Unknown, .detail = "scope ambiguous"},
    };

    EXPECT_EQ(aggregate_status(checks), CheckStatus::Fail);
}

TEST(VerificationEngineTest, AggregateStatusReturnsUnknownWhenNoFailButUnknownExists) {
    const std::vector<VerificationCheck> checks = {
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "ok"},
        VerificationCheck{.name = "scope", .status = CheckStatus::Unknown, .detail = "scope ambiguous"},
    };

    EXPECT_EQ(aggregate_status(checks), CheckStatus::Unknown);
}

TEST(VerificationEngineTest, RunAllReturnsPassWhenAllVerifiersPass) {
    VerificationEngine engine;
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "schema valid"}));
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "reference_integrity", .status = CheckStatus::Pass, .detail = "all references resolved"}));
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "permission", .status = CheckStatus::Pass, .detail = "all required permissions present"}));

    const auto target = make_target();
    VerificationContext context{};

    const auto report = engine.run_all(target, context);

    EXPECT_EQ(report.target.artifact_id, "a_option_01");
    EXPECT_EQ(report.target.version_id, "v_option_01");
    EXPECT_EQ(report.status, CheckStatus::Pass);
    ASSERT_EQ(report.checks.size(), 3u);
    EXPECT_TRUE(report.blockers.empty());
}

TEST(VerificationEngineTest, RunAllReturnsFailAndAddsBlockersWhenAnyVerifierFails) {
    VerificationEngine engine;
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "schema valid"}));
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "permission", .status = CheckStatus::Fail, .detail = "capability exec:report_emit fehlt"}));
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "scope", .status = CheckStatus::Pass, .detail = "scope valid"}));

    const auto target = make_target();
    VerificationContext context{};

    const auto report = engine.run_all(target, context);

    EXPECT_EQ(report.status, CheckStatus::Fail);
    ASSERT_EQ(report.checks.size(), 3u);
    ASSERT_EQ(report.blockers.size(), 1u);
    EXPECT_EQ(report.blockers.front(), "permission: capability exec:report_emit fehlt");
}

TEST(VerificationEngineTest, RunAllReturnsUnknownWhenNoFailButUnknownExists) {
    VerificationEngine engine;
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "schema valid"}));
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "scope", .status = CheckStatus::Unknown, .detail = "scope not resolvable from stream_key"}));

    const auto target = make_target();
    VerificationContext context{};

    const auto report = engine.run_all(target, context);

    EXPECT_EQ(report.status, CheckStatus::Unknown);
    ASSERT_EQ(report.checks.size(), 2u);
    ASSERT_EQ(report.blockers.size(), 1u);
    EXPECT_EQ(report.blockers.front(), "unknown: scope not resolvable from stream_key");
}

TEST(VerificationEngineTest, AddVerifierRejectsNullVerifier) {
    VerificationEngine engine;

    EXPECT_THROW(engine.add_verifier(nullptr), std::invalid_argument);
}

TEST(VerificationEngineTest, MakeVerificationReportArtifactBuildsVerificationArtifact) {
    const auto target = make_target();

    VerificationReportData report{};
    report.target = ArtifactRef{
        .artifact_id = target.artifact_id,
        .version_id = target.version_id,
    };
    report.status = CheckStatus::Pass;
    report.checks = {
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "schema valid"}
    };

    const auto artifact = make_verification_report_artifact(
        target,
        report,
        ActorRef{.actor_type = "system", .id = "verification-engine"},
        SourceRef{.kind = "internal", .ref = "verification"},
        TrustInfo{.level = "high", .source_class = "system"},
        "a_verification_01",
        "v_verification_01",
        target.stream_key,
        "2026-04-20T12:05:00Z");

    EXPECT_EQ(artifact.type, "verification_report");
    EXPECT_EQ(artifact.schema_id, "arcs.verification_report.v1");
    EXPECT_EQ(artifact.stream_key, target.stream_key);
    EXPECT_EQ(artifact.payload.at("status"), "pass");
    EXPECT_EQ(artifact.payload.at("target").at("artifact_id"), target.artifact_id);
}
