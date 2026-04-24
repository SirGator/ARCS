#include "execution/policy_update_executor.hpp"

#include "artifact/artifact.hpp"
#include "artifact/ids.hpp"
#include "execution/action.hpp"
#include "execution/execution_result.hpp"

#include "store/store.hpp"

#include <nlohmann/json.hpp>
#include <stdexcept>
#include <string>
#include <vector>

#include "store/commit.hpp"

namespace arcs::execution {

namespace {

std::string require_string(const nlohmann::json& j, const char* key)
{
    if (!j.contains(key) || !j.at(key).is_string()) {
        throw std::invalid_argument(std::string("missing string field: ") + key);
    }

    return j.at(key).get<std::string>();
}

} // namespace

PolicyUpdateExecutor::PolicyUpdateExecutor(arcs::store::IStore& store)
    : store_(store)
{
}

ExecutionResult PolicyUpdateExecutor::execute(
    const Action& action,
    const ExecutionContext& ctx
) {
    (void)ctx;

    const ActionRef ref{
        .artifact_id = action.artifact_id,
        .version_id = action.version_id,
    };

    try {
        if (action.payload.type != handles_action_type()) {
            throw std::invalid_argument("PolicyUpdateExecutor only accepts policy_update actions");
        }

        if (!action.payload.params.is_object()) {
            throw std::invalid_argument("missing params object");
        }

        const auto& params = action.payload.params;

        if (!params.contains("new_policy") || !params.at("new_policy").is_object()) {
            throw std::invalid_argument("missing params.new_policy object");
        }

        arcs::artifact::ArtifactVersion policy{};
        policy.artifact_id = require_string(params, "policy_artifact_id");
        policy.version_id  = require_string(params, "policy_version_id");
        policy.version     = params.value("policy_version", 1);

        policy.type = "policy";
        policy.schema_id = "arcs.policy.v1";
        policy.schema_version = 1;

        policy.stream_key = "policy:core";

        policy.payload = params.at("new_policy");

        arcs::event::Event event{};
        event.event_id = require_string(params, "event_id");
        event.event_type = "artifact_committed";
        event.stream_key = policy.stream_key;

        event.refs.push_back({
            policy.artifact_id,
            policy.version_id,
            "target"
        });

        event.refs.push_back({
            action.artifact_id,
            action.version_id,
            "parent"
        });

        arcs::store::commit::CommitBundle bundle{};
        bundle.versions.push_back({policy, std::nullopt});
        bundle.events.push_back(event);

        store_.commit(bundle);

        auto result = ExecutionResult::success(ref);
        result.logs.push_back({"policy update committed", ""});
        return result;
    }
    catch (const std::exception& e) {
        auto result = ExecutionResult::fail(ref, e.what());
        result.logs.push_back({e.what(), ""});
        return result;
    }
}

std::string PolicyUpdateExecutor::handles_action_type() const
{
    return "policy_update";
}

} // namespace arcs::execution
