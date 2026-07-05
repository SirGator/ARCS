/**
 * @file approval_state_reducer.cpp
 * @brief Implements ApprovalStateReducer::reduce.
 */
#include "reducer/approval_state_reducer.hpp"

namespace arcs::reducer {

/**
 * @brief Scans "approval" artifacts and derives the latest decision and
 * policy reference, marking the state valid when the last seen decision is
 * "approve". The policy reference may come from a raw string or from a
 * "version_id"/"artifact_id" field of an object payload.
 * @param artifacts Artifact history to reduce over.
 * @return The resulting ApprovalState.
 */
ApprovalState ApprovalStateReducer::reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts)
{
    ApprovalState state{};

    for (const auto& artifact : artifacts) {
        if (artifact.type != "approval") {
            continue;
        }

        if (artifact.payload.contains("decision") && artifact.payload["decision"].is_string()) {
            state.decision = artifact.payload["decision"].get<std::string>();
        }

        if (artifact.payload.contains("policy_ref")) {
            const auto& policy_ref = artifact.payload["policy_ref"];

            if (policy_ref.is_string()) {
                state.policy_ref = policy_ref.get<std::string>();
            } else if (policy_ref.is_object()) {
                if (policy_ref.contains("version_id") && policy_ref["version_id"].is_string()) {
                    state.policy_ref = policy_ref["version_id"].get<std::string>();
                } else if (policy_ref.contains("artifact_id") && policy_ref["artifact_id"].is_string()) {
                    state.policy_ref = policy_ref["artifact_id"].get<std::string>();
                }
            }
        }

        state.valid = (state.decision == "approve");
    }

    return state;
}

} // namespace arcs::reducer
