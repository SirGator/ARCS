#include <gtest/gtest.h>

#include "artifact/factory.hpp"

TEST(ArtifactFactoryTest, CreatesConsistentBaseFields)
{
    const auto artifact = arcs::artifact::factory::make_base_artifact(
        "input",
        "arcs.input.v1",
        "session:test",
        "human",
        "user:test",
        "chat",
        "cli",
        "high",
        "human");

    EXPECT_FALSE(artifact.artifact_id.empty());
    EXPECT_FALSE(artifact.version_id.empty());
    EXPECT_EQ(artifact.version, 1);
    EXPECT_EQ(artifact.stream_key, "session:test");
    EXPECT_EQ(artifact.type, "input");
    EXPECT_EQ(artifact.schema_id, "arcs.input.v1");
    EXPECT_EQ(artifact.created_by.actor_type, "human");
    EXPECT_EQ(artifact.source.ref, "cli");
    EXPECT_EQ(artifact.trust.level, "high");
}
