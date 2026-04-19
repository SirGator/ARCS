#include <gtest/gtest.h>

#include <vector>
#include <utility>

#include "artifact/artifact.hpp"
#include "reducer/task_state.hpp"
#include "reducer/task_state_reducer.hpp"

namespace arcs::reducer {
namespace {

arcs::artifact::ArtifactVersion make_artifact(
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& type,
    nlohmann::json payload = nlohmann::json::object()) {
    arcs::artifact::ArtifactVersion artifact{};
    artifact.artifact_id = artifact_id;
    artifact.version_id = version_id;
    artifact.version = 1;
    artifact.type = type;
    artifact.schema_id = "test." + type + ".v1";
    artifact.schema_version = 1;
    artifact.created_at = "2026-01-01T00:00:00Z";
    artifact.created_by = arcs::artifact::ActorRef{"system", "test"};
    artifact.source = arcs::artifact::SourceRef{"internal", "test"};
    artifact.trust = arcs::artifact::TrustInfo{"high", "system"};
    artifact.stream_key = "task_id:t_replay";
    artifact.tags = {};
    artifact.payload = std::move(payload);
    return artifact;
}

std::vector<arcs::artifact::ArtifactVersion> rebuild_from_event_log(
    const std::vector<arcs::artifact::ArtifactVersion>& event_log_snapshot) {
    std::vector<arcs::artifact::ArtifactVersion> replayed;
    replayed.reserve(event_log_snapshot.size());

    for (const auto& artifact : event_log_snapshot) {
        replayed.push_back(artifact);
    }

    return replayed;
}

} // namespace

TEST(ReplayTest, SameLogProducesIdenticalTaskState) {
    TaskStateReducer reducer;

    const std::vector<arcs::artifact::ArtifactVersion> live_log{
        make_artifact("a_task_1", "v_task_1", "task"),
        make_artifact("a_option_1", "v_option_1", "option"),
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"}
            }),
        make_artifact(
            "a_exec_1",
            "v_exec_1",
            "execution_result",
            {
                {"status", "success"}
            })
    };

    const TaskState live_state = reducer.reduce(live_log);

    const std::vector<arcs::artifact::ArtifactVersion> replayed_log = rebuild_from_event_log(live_log);
    const TaskState replay_state = reducer.reduce(replayed_log);

    EXPECT_EQ(live_state.status, replay_state.status);
    EXPECT_EQ(live_state.option_ids, replay_state.option_ids);
    EXPECT_EQ(live_state.approval_ids, replay_state.approval_ids);
}

TEST(ReplayTest, OrderMattersButReplayPreservesOrderAndState) {
    TaskStateReducer reducer;

    const std::vector<arcs::artifact::ArtifactVersion> live_log{
        make_artifact("a_option_1", "v_option_1", "option"),
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"}
            }),
        make_artifact(
            "a_approval_2",
            "v_approval_2",
            "approval",
            {
                {"decision", "revoke"}
            })
    };

    const TaskState live_state = reducer.reduce(live_log);

    const std::vector<arcs::artifact::ArtifactVersion> replayed_log = rebuild_from_event_log(live_log);
    const TaskState replay_state = reducer.reduce(replayed_log);

    EXPECT_EQ(live_state.status, "blocked");
    EXPECT_EQ(live_state.status, replay_state.status);
    EXPECT_EQ(live_state.option_ids, replay_state.option_ids);
    EXPECT_EQ(live_state.approval_ids, replay_state.approval_ids);
}

} // namespace arcs::reducer
