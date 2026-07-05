#include <gtest/gtest.h>

#include "core/commit/commit_service.hpp"
#include "core/intake/artifact_intake_service.hpp"
#include "core/runtime/runtime_support.hpp"
#include "ingress/quarantine.hpp"
#include "store/store_memory.hpp"

TEST(ArtifactIntakeServiceTest, AcceptsValidSubmissionAndCommitsArtifact)
{
    arcs::store::StoreMemory store;
    arcs::core::commit::CommitService commit_service;
    arcs::core::intake::ArtifactIntakeService intake(
        store,
        arcs::core::runtime::default_payload_schema_registry(),
        commit_service);

    const auto result = intake.accept(arcs::core::intake::AdapterSubmission{
        .adapter_id = "input_cli",
        .adapter_kind = arcs::adapters::AdapterKind::Input,
        .schema_id = "arcs.ingress_event.v1",
        .artifact_type = "ingress_event",
        .stream_key = "session:test",
        .payload = nlohmann::json{
            {"raw_text", "hello"},
            {"source_kind", "chat"},
            {"source_ref", "cli"},
            {"actor_id", "user:test"},
        },
        .metadata = nlohmann::json::object(),
        .actor_type = "human",
        .actor_id = "user:test",
        .source_kind = "chat",
        .source_ref = "cli",
    });

    ASSERT_TRUE(result.accepted);
    ASSERT_TRUE(result.artifact_ref.has_value());
    EXPECT_TRUE(store.has_artifact(result.artifact_ref->artifact_id));
}

TEST(ArtifactIntakeServiceTest, RejectsInvalidSubmissionIntoQuarantine)
{
    arcs::store::StoreMemory store;
    arcs::ingress::QuarantineStore quarantine;
    arcs::core::commit::CommitService commit_service;
    arcs::core::intake::ArtifactIntakeService intake(
        store,
        arcs::core::runtime::default_payload_schema_registry(),
        commit_service);

    const auto result = intake.accept(arcs::core::intake::AdapterSubmission{
        .adapter_id = "input_cli",
        .adapter_kind = arcs::adapters::AdapterKind::Input,
        .schema_id = "arcs.ingress_event.v1",
        .artifact_type = "ingress_event",
        .stream_key = "session:test",
        .payload = nlohmann::json{{"raw_text", "hello"}},
        .metadata = nlohmann::json::object(),
        .actor_type = "human",
        .actor_id = "user:test",
        .source_kind = "chat",
        .source_ref = "cli",
    }, &quarantine);

    EXPECT_FALSE(result.accepted);
    EXPECT_FALSE(result.artifact_ref.has_value());
    EXPECT_FALSE(result.rejection_reason.empty());
    EXPECT_EQ(quarantine.count(), 1u);
}
