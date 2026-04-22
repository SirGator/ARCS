#include <gtest/gtest.h>

#include "approval.hpp"
#include "artifact/artifact.hpp"

using arcs::artifact::ArtifactVersion;
using namespace arcs::approval;

namespace {

bool policy_ref_matches_current_head(const ArtifactVersion& approval,
                                     const ArtifactVersion& current_policy_head) {
    const auto& pref = approval.payload["policy_ref"];
    return pref["artifact_id"] == current_policy_head.artifact_id &&
           pref["version_id"]  == current_policy_head.version_id;
}

TEST(PolicyDriftTest, ApprovalBoundToOldPolicyVersionBlocksWhenPolicyHeadChanges) {
    ArtifactVersion option{};
    option.artifact_id = "a_option_1";
    option.version_id  = "v_option_1";
    option.type        = "option";
    option.stream_key  = "task_id:t_01";

    ArtifactVersion policy_v1{};
    policy_v1.artifact_id = "a_policy_1";
    policy_v1.version_id  = "v_policy_1";
    policy_v1.type        = "policy";
    policy_v1.schema_id   = "arcs.policy.v1";

    ArtifactVersion policy_v2{};
    policy_v2.artifact_id = "a_policy_1";   // gleiche Policy
    policy_v2.version_id  = "v_policy_2";   // neuer Head
    policy_v2.type        = "policy";
    policy_v2.schema_id   = "arcs.policy.v1";

    ArtifactVersion approval{};
    approval.type      = "approval";
    approval.schema_id = "arcs.approval.v1";
    approval.payload = {
        {"target_option", {
            {"artifact_id", option.artifact_id},
            {"version_id",  option.version_id}
        }},
        {"policy_ref", {
            {"artifact_id", policy_v1.artifact_id},
            {"version_id",  policy_v1.version_id}
        }},
        {"decision", "approve"}
    };

    EXPECT_FALSE(policy_ref_matches_current_head(approval, policy_v2));
}
    
TEST(PolicyDriftTest, ApprovalStillValidWhenPolicyHeadIsSameVersion) {
    ArtifactVersion policy_v1{};
    policy_v1.artifact_id = "a_policy_1";
    policy_v1.version_id  = "v_policy_1";

    ArtifactVersion approval{};
    approval.type = "approval";
    approval.payload = {
        {"policy_ref", {
            {"artifact_id", "a_policy_1"},
            {"version_id",  "v_policy_1"}
        }}
    };

    EXPECT_TRUE(policy_ref_matches_current_head(approval, policy_v1));
}

} // namespace
