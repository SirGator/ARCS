#include <gtest/gtest.h>

#include <sstream>
#include <string>

#include "artifact/json.hpp"
#include "input/input_adapter.hpp"

TEST(InputArtifactTest, NormalizesCliTextIntoArtifact)
{
    std::istringstream in("approval=yes permission=yes\n");

    arcs::input::CliTextInputAdapter input_adapter;
    const auto artifact = input_adapter.read_artifact(in, "cli", "session:test");

    EXPECT_EQ(artifact.type, "input");
    EXPECT_EQ(artifact.schema_id, "arcs.input.v1");
    EXPECT_EQ(artifact.version, 1);
    EXPECT_EQ(artifact.stream_key, "session:test");
    EXPECT_EQ(artifact.payload.at("raw_text").get<std::string>(), "approval=yes permission=yes");
    EXPECT_EQ(artifact.payload.at("source").get<std::string>(), "cli");
    EXPECT_EQ(artifact.source.kind, "chat");
    EXPECT_EQ(artifact.source.ref, "cli");
    EXPECT_EQ(artifact.created_by.actor_type, "human");
}
