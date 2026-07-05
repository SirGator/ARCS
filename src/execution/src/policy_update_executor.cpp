#include "execution/policy_update_executor.hpp"

#include "artifact/artifact.hpp"
#include "artifact/factory.hpp"
#include "artifact/ids.hpp"
#include "execution/action.hpp"
#include "execution/execution_result.hpp"

#include "store/store.hpp"

#include <nlohmann/json.hpp>
#include <stdexcept>
#include <string>
#include <vector>

#include "store/commit.hpp"

/**
 * @file policy_update_executor.cpp
 * @brief Implements PolicyUpdateExecutor::execute, which builds a new
 *        policy artifact version from an action's parameters, records a
 *        corresponding "artifact_committed" event, and commits both to
 *        the artifact store.
 */

namespace arcs::execution {

namespace {

/**
 * @brief Fetch a required string field from a JSON object.
 * @param j JSON object expected to contain @p key as a string.
 * @param key Name of the field to read.
 * @return The string value at @p key.
 * @throws std::invalid_argument if @p key is missing or not a string.
 */
std::string require_string(const nlohmann::json& j, const char* key)
{
    if (!j.contains(key) || !j.at(key).is_string()) {
        throw std::invalid_argument(std::string("missing string field: ") + key);
    }

    return j.at(key).get<std::string>();
}

} // namespace

/** @brief Constructs the executor, retaining a reference to the target store. */
PolicyUpdateExecutor::PolicyUpdateExecutor(arcs::store::IStore& store)
    : store_(store)
{
}

/**
 * @brief Applies a "policy_update" action: validates its type and
 *        parameters, constructs a new policy artifact version and a
 *        matching "artifact_committed" event, and commits both as a
 *        single bundle to the store.
 * @param action Action to execute; payload.type must equal
 *               handles_action_type() ("policy_update") and must contain
 *               params.new_policy, params.policy_artifact_id,
 *               params.policy_version_id, and params.event_id.
 * @param ctx Execution context (unused by this executor).
 * @return A success ExecutionResult once the commit succeeds, or a
 *         failure result carrying the validation/commit error message.
 */
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

        arcs::artifact::ArtifactVersion policy = arcs::artifact::factory::make_base_artifact(
            "policy",
            "arcs.policy.v1",
            "policy:core",
            "system",
            "policy_update_executor",
            "internal",
            "policy_update",
            "high",
            "system");
        policy.artifact_id = require_string(params, "policy_artifact_id");
        policy.version_id = require_string(params, "policy_version_id");
        policy.version = params.value("policy_version", 1);

        policy.payload = params.at("new_policy");
        policy.provenance.parents = {action.artifact_id};
        policy.provenance.rules_applied = {"policy_update_executor"};
        policy.provenance.transform = "policy_update";

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

/** @brief Returns "policy_update", the action type this executor handles. */
std::string PolicyUpdateExecutor::handles_action_type() const
{
    return "policy_update";
}

} // namespace arcs::execution
