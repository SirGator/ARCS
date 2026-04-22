#include <memory>

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

ArtifactVersion make_option_with_ambiguous_scope() {
    ArtifactVersion target{};
    target.artifact_id = "a_option_ambiguous";
    target.version_id = "v_option_ambiguous";
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

    // Absichtlich leer, damit Scope nicht eindeutig ableitbar ist.
    target.stream_key = "";

    target.payload = {
        {"title", "Generate report"},
        {"requires_permissions", {"exec:report_emit"}},
        {"required_scopes", {"task_id:t_01"}},
        {"steps", {
            {
                {"kind", "emit_report"},
                {"params", {{"format", "pdf"}}}
            }
        }}
    };

    return target;
}

bool is_blocked_by_verification(const VerificationReportData& report) {
    return report.status == CheckStatus::Fail ||
           report.status == CheckStatus::Unknown;
}

bool can_materialize_action(const VerificationReportData& report) {
    return report.status == CheckStatus::Pass;
}

} // namespace

TEST(UnknownBlocksTest, UnknownAggregateMeansBlocked) {
    VerificationEngine engine;
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "schema valid"}));
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "permission", .status = CheckStatus::Pass, .detail = "all required permissions present"}));
    engine.add_verifier(std::make_shared<StubVerifier>(
        VerificationCheck{.name = "scope", .status = CheckStatus::Unknown, .detail = "scope not resolvable from stream_key"}));

    const auto target = make_option_with_ambiguous_scope();
    VerificationContext context{};

    const auto report = engine.run_all(target, context);

    EXPECT_EQ(report.status, CheckStatus::Unknown);
    EXPECT_TRUE(is_blocked_by_verification(report));
    EXPECT_FALSE(can_materialize_action(report));

    ASSERT_EQ(report.blockers.size(), 1u);
    EXPECT_EQ(report.blockers.front(), "unknown: scope not resolvable from stream_key");
}

TEST(UnknownBlocksTest, UnknownFromScopeVerifierBlocksOption) {
    ScopeVerifier verifier;

    auto target = make_option_with_ambiguous_scope();

    VerificationContext context{};
    context.permissions.capabilities = {"exec:report_emit"};
    context.permissions.scopes = {"task_id:t_01"};

    const auto check = verifier.check(target, context);

    EXPECT_EQ(check.name, "scope");
    EXPECT_EQ(check.status, CheckStatus::Unknown);
    EXPECT_EQ(check.detail, "scope not resolvable from stream_key");
}

TEST(UnknownBlocksTest, UnknownTakesPriorityOverPassAndStillBlocks) {
    const std::vector<VerificationCheck> checks = {
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "ok"},
        VerificationCheck{.name = "reference_integrity", .status = CheckStatus::Pass, .detail = "ok"},
        VerificationCheck{.name = "scope", .status = CheckStatus::Unknown, .detail = "ambiguous scope"},
    };

    const auto status = aggregate_status(checks);

    EXPECT_EQ(status, CheckStatus::Unknown);
}

TEST(UnknownBlocksTest, FailStillWinsOverUnknownInAggregation) {
    const std::vector<VerificationCheck> checks = {
        VerificationCheck{.name = "schema", .status = CheckStatus::Pass, .detail = "ok"},
        VerificationCheck{.name = "scope", .status = CheckStatus::Unknown, .detail = "ambiguous scope"},
        VerificationCheck{.name = "permission", .status = CheckStatus::Fail, .detail = "capability exec:report_emit fehlt"},
    };

    const auto status = aggregate_status(checks);

    EXPECT_EQ(status, CheckStatus::Fail);
}
