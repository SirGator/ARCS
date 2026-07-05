/**
 * @file execution_service.cpp
 * @brief Implements ExecutionService, the pipeline stage responsible for actually
 *        executing an approved action artifact. Converts the artifact into an
 *        execution::Action, runs it through a ReportEmitExecutor backed by an
 *        in-memory idempotency store, and (on success) produces an
 *        "execution_result" artifact recording the outcome.
 */

#include "core/services/execution_service.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <map>
#include <optional>
#include <sstream>

#include "artifact/factory.hpp"
#include "execution/idempotency.hpp"
#include "execution/report_emit_executor.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief In-memory implementation of IIdempotencyStore used to deduplicate
 *        execution results within a single ExecutionService call, keyed by action ID.
 */
class KernelIdempotencyStore final : public arcs::execution::IIdempotencyStore {
public:
    /**
     * @brief Checks whether a result has already been recorded for the given action.
     * @param action_id The action identifier to look up.
     * @return True if a result is already stored for this action ID.
     */
    bool has(const std::string& action_id) const override
    {
        return results_.find(action_id) != results_.end();
    }

    /**
     * @brief Retrieves a previously stored execution result for an action, if any.
     * @param action_id The action identifier to look up.
     * @return The stored ExecutionResult, or std::nullopt if none is recorded.
     */
    std::optional<arcs::execution::ExecutionResult> get(const std::string& action_id) const override
    {
        const auto it = results_.find(action_id);
        if (it == results_.end()) {
            return std::nullopt;
        }

        return it->second;
    }

    /**
     * @brief Stores (or overwrites) the execution result for the given action ID.
     * @param action_id The action identifier to store the result under.
     * @param result The execution result to store.
     */
    void put(const std::string& action_id, const arcs::execution::ExecutionResult& result) override
    {
        results_[action_id] = result;
    }

private:
    std::map<std::string, arcs::execution::ExecutionResult> results_;
};

/**
 * @brief Returns the current UTC time formatted as an ISO-8601 string (YYYY-MM-DDTHH:MM:SSZ).
 * @return The formatted current UTC timestamp.
 */
std::string utc_now()
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());
    std::tm tm{};

#if defined(_WIN32)
    gmtime_s(&tm, &now_time_t);
#else
    gmtime_r(&now_time_t, &tm);
#endif

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
    return out.str();
}

/**
 * @brief Constructs a base artifact version stamped with the current UTC time,
 *        forwarding all fields to arcs::artifact::factory::make_base_artifact.
 * @param type The artifact type.
 * @param schema_id The schema identifier the artifact payload conforms to.
 * @param stream_key The stream key the artifact belongs to.
 * @param actor_type The type of actor creating the artifact.
 * @param actor_id The identifier of the actor creating the artifact.
 * @param source_kind The kind of source that produced the artifact.
 * @param source_ref A reference describing the artifact's source.
 * @param trust_level The trust level assigned to the artifact.
 * @param trust_source_class The trust classification of the source.
 * @return The newly constructed base artifact version.
 */
arcs::artifact::ArtifactVersion make_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& actor_type,
    const std::string& actor_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class)
{
    return arcs::artifact::factory::make_base_artifact(
        type,
        schema_id,
        stream_key,
        actor_type,
        actor_id,
        source_kind,
        source_ref,
        trust_level,
        trust_source_class,
        utc_now());
}

/**
 * @brief Converts an action artifact into the execution::Action structure expected
 *        by the executor, extracting action_id, type, params, safety_level,
 *        idempotency_key, and required_permissions from the artifact's payload.
 * @param artifact The action artifact version to convert.
 * @return The equivalent execution::Action.
 */
arcs::execution::Action to_execution_action(const arcs::artifact::ArtifactVersion& artifact)
{
    arcs::execution::Action action{};
    action.artifact_id = artifact.artifact_id;
    action.version_id = artifact.version_id;
    action.payload.action_id = artifact.payload.value("action_id", std::string{});
    action.payload.type = artifact.payload.value("type", std::string{});
    action.payload.params = artifact.payload.value("params", nlohmann::json::object());
    action.payload.safety_level = artifact.payload.value("safety_level", std::string{});
    action.payload.idempotency_key = artifact.payload.value("idempotency_key", std::string{});
    if (artifact.payload.contains("required_permissions") && artifact.payload.at("required_permissions").is_array()) {
        for (const auto& entry : artifact.payload.at("required_permissions")) {
            if (entry.is_string()) {
                action.payload.required_permissions.push_back(entry.get<std::string>());
            }
        }
    }
    return action;
}

/**
 * @brief Builds an "execution_result" artifact summarizing the outcome of running
 *        an action: its status (success/fail/timeout/cancelled), exit code, error
 *        message, and collected logs.
 * @param action The action artifact that was executed.
 * @param result The execution result produced by the executor.
 * @return The constructed execution_result artifact version, referencing the action.
 */
arcs::artifact::ArtifactVersion make_execution_result_artifact(
    const arcs::artifact::ArtifactVersion& action,
    const arcs::execution::ExecutionResult& result)
{
    auto artifact = make_artifact(
        "execution_result",
        "arcs.execution_result.v1",
        action.stream_key,
        "executor",
        "report_emit_executor",
        "internal",
        "execution",
        "high",
        "system");

    nlohmann::json logs = nlohmann::json::array();
    for (const auto& log : result.logs) {
        logs.push_back({
            {"message", log.message},
            {"timestamp", log.timestamp},
        });
    }

    std::string status = "fail";
    switch (result.status) {
        case arcs::execution::ExecutionStatus::Success:
            status = "success";
            break;
        case arcs::execution::ExecutionStatus::Fail:
            status = "fail";
            break;
        case arcs::execution::ExecutionStatus::Timeout:
            status = "timeout";
            break;
        case arcs::execution::ExecutionStatus::Cancelled:
            status = "cancelled";
            break;
    }

    artifact.payload = {
        {"action_ref", {{"artifact_id", action.artifact_id}, {"version_id", action.version_id}}},
        {"status", status},
        {"exit_code", result.exit_code},
        {"error_message", result.error_message},
        {"logs", logs},
    };
    artifact.provenance.parents = {action.artifact_id};
    artifact.provenance.rules_applied = {"report_emit_executor"};
    artifact.provenance.transform = "execute_report_emit";
    return artifact;
}

} // namespace

/**
 * @brief Executes a "report_emit" action artifact via ReportEmitExecutor and, if
 *        successful, produces a corresponding execution_result artifact.
 * @param action_artifact The action artifact to execute.
 * @param execution_context The execution context passed through to the executor.
 * @return The ExecutionOutcome containing the raw execution result and, on
 *         success, the generated execution_result artifact.
 */
ExecutionOutcome ExecutionService::execute_report_action(
    const arcs::artifact::ArtifactVersion& action_artifact,
    const arcs::execution::ExecutionContext& execution_context) const
{
    KernelIdempotencyStore idempotency_store;
    arcs::execution::ReportEmitExecutor executor(idempotency_store);
    const auto execution_action = to_execution_action(action_artifact);
    const auto execution_result = executor.execute(execution_action, execution_context);

    ExecutionOutcome outcome{.result = execution_result};
    if (execution_result.status == arcs::execution::ExecutionStatus::Success) {
        outcome.result_artifact = make_execution_result_artifact(action_artifact, execution_result);
    }
    return outcome;
}

} // namespace arcs::core::services
