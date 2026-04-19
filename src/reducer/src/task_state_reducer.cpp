#include "reducer/task_state_reducer.hpp"

namespace arcs::reducer {

TaskState TaskStateReducer::reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts)
{
    TaskState state{};
    bool has_option = false;
    bool approved = false;
    bool executed = false;

    for (const auto& artifact : artifacts) {
        if (artifact.type == "option") {
            has_option = true;
            state.option_ids.push_back(artifact.artifact_id);
        } else if (artifact.type == "approval") {
            state.approval_ids.push_back(artifact.artifact_id);

            if (artifact.payload.contains("decision") && artifact.payload["decision"].is_string()) {
                const auto decision = artifact.payload["decision"].get<std::string>();

                if (decision == "approve") {
                    approved = true;
                } else if (decision == "revoke" || decision == "reject") {
                    approved = false;
                }
            }
        } else if (artifact.type == "execution_result") {
            executed = true;
        }
    }

    if (!has_option) {
        state.status = "draft";
    } else if (executed) {
        state.status = "executed";
    } else if (approved) {
        state.status = "approved";
    } else {
        state.status = "blocked";
    }

    return state;
}

} // namespace arcs::reducer
