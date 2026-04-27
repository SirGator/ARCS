#include "core/flow.hpp"

#include <algorithm>
#include <cctype>
#include <chrono>
#include <map>
#include <memory>
#include <optional>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "artifact/factory.hpp"
#include "artifact/json.hpp"
#include "artifact/ids.hpp"
#include "approval.hpp"
#include "event/event.hpp"
#include "core/system_logger.hpp"
#include "execution/action.hpp"
#include "materializer.hpp"
#include "execution/report_emit_executor.hpp"
#include "execution/idempotency.hpp"
#include "store/commit.hpp"
#include "store/store_memory.hpp"
#include "verification/authority_verifier.hpp"
#include "verification/verifier.hpp"

namespace arcs::core {

namespace {

using ArtifactVersion = arcs::artifact::ArtifactVersion;
using CommitBundle = arcs::store::commit::CommitBundle;
using PendingVersion = arcs::store::commit::PendingVersion;
using Event = arcs::event::Event;
using EventRef = arcs::event::EventRef;

struct ParsedInput {
    bool approval_yes{false};
    bool permission_yes{false};
    bool policy_drift{false};
};

class KernelIdempotencyStore final : public arcs::execution::IIdempotencyStore {
public:
    bool has(const std::string& action_id) const override
    {
        return results_.find(action_id) != results_.end();
    }

    std::optional<arcs::execution::ExecutionResult> get(const std::string& action_id) const override
    {
        const auto it = results_.find(action_id);
        if (it == results_.end()) {
            return std::nullopt;
        }

        return it->second;
    }

    void put(const std::string& action_id, const arcs::execution::ExecutionResult& result) override
    {
        results_[action_id] = result;
    }

private:
    std::map<std::string, arcs::execution::ExecutionResult> results_;
};

std::string to_lower_copy(std::string value)
{
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return value;
}

std::map<std::string, std::string> parse_key_values(const std::string& input)
{
    std::map<std::string, std::string> values;
    std::istringstream stream(input);
    std::string token;

    while (stream >> token) {
        const auto pos = token.find('=');
        if (pos == std::string::npos) {
            continue;
        }

        auto key = to_lower_copy(token.substr(0, pos));
        auto value = to_lower_copy(token.substr(pos + 1));
        if (!key.empty() && !value.empty()) {
            values.emplace(std::move(key), std::move(value));
        }
    }

    return values;
}

bool is_yes(const std::map<std::string, std::string>& values, const std::string& key)
{
    const auto it = values.find(key);
    return it != values.end() && it->second == "yes";
}

ParsedInput parse_input(const std::string& input)
{
    ParsedInput parsed{};
    const auto values = parse_key_values(input);
    parsed.approval_yes = is_yes(values, "approval");
    parsed.permission_yes = is_yes(values, "permission");
    parsed.policy_drift = is_yes(values, "policy_drift");
    return parsed;
}

std::string utc_now()
{
    return "2026-04-26T00:00:00Z";
}

ArtifactVersion make_artifact(
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

Event make_head_advanced_event(const ArtifactVersion& artifact)
{
    Event event{};
    event.event_id = arcs::artifact::ids::new_event_id();
    event.event_type = "head_advanced";
    event.ts = utc_now();
    event.actor = artifact.created_by;
    event.refs.push_back(EventRef{
        .artifact_id = artifact.artifact_id,
        .version_id = artifact.version_id,
        .role = "target",
    });
    event.stream_key = artifact.stream_key;
    event.payload = {
        {"artifact_type", artifact.type},
        {"schema_id", artifact.schema_id},
    };
    return event;
}

void add_version(CommitBundle& bundle, const ArtifactVersion& artifact)
{
    bundle.versions.push_back(PendingVersion{artifact, std::nullopt});
    bundle.events.push_back(make_head_advanced_event(artifact));
}

std::string first_blocker_or(const arcs::verification::VerificationReportData& report, const std::string& fallback)
{
    if (!report.blockers.empty()) {
        return report.blockers.front();
    }

    return fallback;
}

ArtifactVersion make_execution_result_artifact(
    const ArtifactVersion& action,
    const arcs::execution::ExecutionResult& result)
{
    ArtifactVersion artifact = make_artifact(
        "execution_result",
        "arcs.execution_result.v1",
        action.stream_key,
        "system",
        "report_emit_executor",
        "internal",
        "execution",
        "high",
        "system");

    artifact.payload = nlohmann::json{
        {"action_ref", {
            {"artifact_id", result.action_ref.artifact_id},
            {"version_id", result.action_ref.version_id},
        }},
        {"status", result.status == arcs::execution::ExecutionStatus::Success ? "success" :
                   result.status == arcs::execution::ExecutionStatus::Fail ? "fail" :
                   result.status == arcs::execution::ExecutionStatus::Timeout ? "timeout" :
                   "cancelled"},
        {"exit_code", result.exit_code},
        {"error_message", result.error_message},
        {"logs", nlohmann::json::array()},
    };

    for (const auto& log : result.logs) {
        artifact.payload["logs"].push_back({
            {"message", log.message},
            {"timestamp", log.timestamp},
        });
    }

    artifact.provenance.parents = {action.artifact_id};
    artifact.provenance.rules_applied = {"report_emit_executor"};
    artifact.provenance.transform = "execute_report_emit";
    return artifact;
}

arcs::execution::Action to_execution_action(const ArtifactVersion& artifact)
{
    arcs::execution::Action action{};
    action.artifact_id = artifact.artifact_id;
    action.version_id = artifact.version_id;
    action.payload.action_id = artifact.payload.value("action_id", std::string{});
    action.payload.type = artifact.payload.value("type", std::string{});
    action.payload.params = artifact.payload.value("params", nlohmann::json::object());
    action.payload.safety_level = artifact.payload.value("safety_level", std::string{});
    action.payload.idempotency_key = artifact.payload.value("idempotency_key", std::string{});

    if (artifact.payload.contains("required_permissions") && artifact.payload["required_permissions"].is_array()) {
        for (const auto& permission : artifact.payload["required_permissions"]) {
            if (permission.is_string()) {
                action.payload.required_permissions.push_back(permission.get<std::string>());
            }
        }
    }

    return action;
}

ArtifactVersion make_decision_artifact(
    const ArtifactVersion& option,
    const arcs::verification::VerificationReportData& report,
    const std::string& status,
    const std::string& reason,
    const std::string& approval_artifact_id,
    const std::string& action_artifact_id,
    const std::string& execution_result_artifact_id)
{
    ArtifactVersion artifact = make_artifact(
        "decision",
        "arcs.decision.v1",
        option.stream_key,
        "system",
        "kernel",
        "internal",
        "decision",
        "high",
        "system");

    artifact.payload = nlohmann::json{
        {"status", status},
        {"reason", reason},
        {"verification_report", {
            {"artifact_id", report.target.artifact_id},
            {"version_id", report.target.version_id},
            {"status", arcs::verification::to_string(report.status)},
        }},
        {"approval_artifact_id", approval_artifact_id},
        {"action_artifact_id", action_artifact_id},
        {"execution_result_artifact_id", execution_result_artifact_id},
    };

    artifact.provenance.parents = {option.artifact_id};
    artifact.provenance.rules_applied = {"kernel_decision"};
    artifact.provenance.transform = "decide";
    return artifact;
}

} // namespace

std::string run_text_flow(const std::string& input)
{
    SystemLogger logger;
    std::ostringstream output;
    output << "input: " << input << '\n';
    logger.ok("input received", input.empty() ? "empty" : "text present");

    if (input.empty()) {
        logger.fail("parse input", "empty input");
        output << logger.format();
        output << "decision: blocked\n";
        output << "reason: empty input\n";
        return output.str();
    }

    const auto values = parse_key_values(input);
    if (values.empty()) {
        logger.fail("parse input", "no key=value pairs found");
        output << logger.format();
        output << "decision: blocked\n";
        output << "reason: missing approval or permission\n";
        return output.str();
    }

    logger.ok("parse input", "key=value pairs parsed");

    const auto parsed = parse_input(input);
    const bool approval_ok = parsed.approval_yes;
    const bool permission_ok = parsed.permission_yes;
    const bool policy_drift = parsed.policy_drift;

    arcs::store::StoreMemory store;
    CommitBundle bundle{};

    const auto ingress = make_artifact(
        "ingress_event",
        "arcs.ingress_event.v1",
        "session:cli",
        "human",
        "user:cli",
        "chat",
        "cli",
        "high",
        "human");
    const auto ingress_payload = nlohmann::json{
        {"raw_text", input},
        {"approval", approval_ok ? "yes" : "no"},
        {"permission", permission_ok ? "yes" : "no"},
        {"policy_drift", policy_drift ? "yes" : "no"},
    };

    ArtifactVersion ingress_event = ingress;
    ingress_event.payload = ingress_payload;
    ingress_event.provenance.rules_applied = {"ingress_normalized"};
    ingress_event.provenance.transform = "normalize_ingress";

    const auto task = make_artifact(
        "task",
        "arcs.task.v1",
        ingress.stream_key,
        "system",
        "kernel",
        "internal",
        "ingress_event",
        "high",
        "system");
    ArtifactVersion task_artifact = task;
    task_artifact.payload = {
        {"request", input},
        {"ingress_ref", {
            {"artifact_id", ingress_event.artifact_id},
            {"version_id", ingress_event.version_id},
        }},
        {"policy_drift", policy_drift ? "yes" : "no"},
    };
    task_artifact.provenance.parents = {ingress_event.artifact_id};
    task_artifact.provenance.rules_applied = {"task_from_ingress"};
    task_artifact.provenance.transform = "derive_task";

    const auto current_policy = make_artifact(
        "policy",
        "arcs.policy.v1",
        "policy:core",
        "system",
        "kernel",
        "internal",
        "policy_bootstrap",
        "high",
        "system");
    ArtifactVersion policy_current = current_policy;
    policy_current.artifact_id = "a_policy_core";
    policy_current.version_id = "v_policy_002";
    policy_current.payload = nlohmann::json{
        {"capabilities", {"exec:report_emit"}},
        {"constraints", nlohmann::json::object()},
        {"verifier_rules", {
            {"hard_checks", {"permission", "policy_drift"}},
            {"soft_checks", nlohmann::json::array()},
        }},
        {"approval_required_for", {"exec:report_emit"}},
    };
    policy_current.provenance.rules_applied = {"policy_bootstrap"};
    policy_current.provenance.transform = "policy_current";

    ArtifactVersion policy_previous = policy_current;
    policy_previous.version_id = "v_policy_001";
    policy_previous.payload["verifier_rules"]["hard_checks"] = {"permission"};
    policy_previous.provenance.transform = "policy_previous";

    const auto policy_ref = policy_drift ? policy_previous : policy_current;

    ArtifactVersion option = make_artifact(
        "option",
        "arcs.option.v1",
        task_artifact.stream_key,
        "system",
        "kernel",
        "internal",
        "task_to_option",
        "high",
        "system");
    option.payload = nlohmann::json{
        {"title", "Generate report"},
        {"request", input},
        {"policy_ref", {
            {"artifact_id", policy_ref.artifact_id},
            {"version_id", policy_ref.version_id},
        }},
        {"requires_permissions", {"exec:report_emit"}},
        {"required_scopes", {task_artifact.stream_key}},
        {"steps", nlohmann::json::array({nlohmann::json{
            {"kind", "emit_report"},
            {"params", {
                {"format", "json"},
                {"sections", {"summary", "risks"}},
            }},
        }})},
    };
    option.provenance.parents = {task_artifact.artifact_id, policy_ref.artifact_id};
    option.provenance.rules_applied = {"materialize_option"};
    option.provenance.transform = "derive_option";

    logger.ok("ingress_event", "artifact created");
    logger.ok("task", "artifact created");
    logger.ok("option", "artifact created");

    if (approval_ok) {
        logger.ok("check approval", "approval=yes");
    } else {
        logger.fail("check approval", "approval missing or not yes");
    }

    if (permission_ok) {
        logger.ok("check permission", "permission=yes");
    } else {
        logger.fail("check permission", "permission missing or not yes");
    }

    if (policy_drift) {
        logger.fail("policy drift", "option bound to stale policy ref");
    } else {
        logger.ok("policy drift", "option policy binding matches current head");
    }

    arcs::verification::VerificationEngine verification_engine;
    verification_engine.add_verifier(std::make_shared<arcs::verification::PermissionVerifier>());
    verification_engine.add_verifier(std::make_shared<arcs::verification::ScopeVerifier>());

    arcs::verification::VerificationContext verification_context{};
    verification_context.permissions.capabilities = permission_ok ? std::vector<std::string>{"exec:report_emit"} : std::vector<std::string>{};
    verification_context.permissions.scopes = {task_artifact.stream_key};

    auto report = verification_engine.run_all(option, verification_context);

    if (approval_ok) {
        report.checks.push_back(arcs::verification::VerificationCheck{
            .name = "approval",
            .status = arcs::verification::CheckStatus::Pass,
            .detail = "approval requested",
        });
    } else {
        report.checks.push_back(arcs::verification::VerificationCheck{
            .name = "approval",
            .status = arcs::verification::CheckStatus::Fail,
            .detail = "missing approval",
        });
    }

    if (policy_drift) {
        report.checks.push_back(arcs::verification::VerificationCheck{
            .name = "policy_drift",
            .status = arcs::verification::CheckStatus::Fail,
            .detail = "option.policy_ref does not match current policy head",
        });
    } else {
        report.checks.push_back(arcs::verification::VerificationCheck{
            .name = "policy_drift",
            .status = arcs::verification::CheckStatus::Pass,
            .detail = "policy head matches option binding",
        });
    }

    report = arcs::verification::make_verification_report(option, std::move(report.checks));

    auto report_artifact = arcs::verification::make_verification_report_artifact(
        option,
        report,
        arcs::artifact::ActorRef{.actor_type = "system", .id = "kernel"},
        arcs::artifact::SourceRef{.kind = "internal", .ref = "verification"},
        arcs::artifact::TrustInfo{.level = "high", .source_class = "system"},
        "a_verification_report",
        "v_verification_report",
        option.stream_key,
        utc_now());

    if (report.status == arcs::verification::CheckStatus::Pass) {
        logger.ok("verification_report", "pass");
    } else {
        logger.fail("verification_report", arcs::verification::to_string(report.status));
    }

    ArtifactVersion approval_artifact{};
    ArtifactVersion action_artifact{};
    ArtifactVersion execution_result_artifact{};
    std::string reason;
    std::string decision_status = "blocked";

    if (report.status != arcs::verification::CheckStatus::Pass) {
        reason = first_blocker_or(report, "verification blocked");
        logger.fail("decision", reason);
    } else {
        arcs::approval::ApprovalGate approval_gate;
        arcs::approval::ApprovalPayload approval_payload{
            .target_option = {option.artifact_id, option.version_id},
            .policy_ref = {policy_current.artifact_id, policy_current.version_id},
            .decision = arcs::approval::ApprovalDecision::Approve,
            .reason = "kernel approval for report_emit",
            .actor = {"human", "user:cli"},
            .timestamp = utc_now(),
            .expires_at = utc_now(),
        };

        approval_artifact = approval_gate.submit(approval_payload);
        approval_artifact.stream_key = option.stream_key;

        logger.ok("approval", "approval artifact created");

        arcs::execution::ActionMaterializer materializer;
        auto actions = materializer.materialize(option, policy_current);
        if (actions.empty()) {
            reason = "no action materialized";
            logger.fail("decision", reason);
        } else {
            action_artifact = actions.front();
            logger.ok("materialize action", "report_emit");

            KernelIdempotencyStore idempotency_store;
            arcs::execution::ReportEmitExecutor executor(idempotency_store);
            arcs::execution::ExecutionContext execution_context{};
            execution_context.approval_id = approval_artifact.artifact_id;
            execution_context.verification_id = report_artifact.artifact_id;
            execution_context.approval_valid = true;
            execution_context.verification_passed = true;
            execution_context.granted_permissions = verification_context.permissions.capabilities;

            const auto execution_action = to_execution_action(action_artifact);
            const auto execution_result = executor.execute(execution_action, execution_context);
            if (execution_result.status == arcs::execution::ExecutionStatus::Success) {
                logger.ok("execute action", "report_emit success");
                execution_result_artifact = make_execution_result_artifact(action_artifact, execution_result);
                decision_status = "not blocked";
                reason = "approval=yes and permission=yes";
                logger.ok("decision", decision_status);
            } else {
                reason = execution_result.error_message.empty() ? "execution blocked" : execution_result.error_message;
                logger.fail("decision", reason);
            }
        }
    }

    if (decision_status == "blocked" && reason.empty()) {
        reason = "missing approval or permission";
    }

    auto decision_artifact = make_decision_artifact(
        option,
        report,
        decision_status,
        reason,
        approval_artifact.artifact_id,
        action_artifact.artifact_id,
        execution_result_artifact.artifact_id);

    bundle.versions.reserve(8);
    add_version(bundle, ingress_event);
    add_version(bundle, task_artifact);
    add_version(bundle, policy_previous);
    add_version(bundle, policy_current);
    add_version(bundle, option);
    add_version(bundle, report_artifact);
    if (!approval_artifact.artifact_id.empty()) {
        add_version(bundle, approval_artifact);
    }
    if (!action_artifact.artifact_id.empty()) {
        add_version(bundle, action_artifact);
    }
    if (!execution_result_artifact.artifact_id.empty()) {
        add_version(bundle, execution_result_artifact);
    }
    add_version(bundle, decision_artifact);

    store.commit(bundle);

    if (decision_status == "not blocked") {
        output << logger.format();
        output << "decision: not blocked\n";
        output << "reason: " << reason << '\n';
        return output.str();
    }

    output << logger.format();
    output << "decision: blocked\n";
    output << "reason: " << reason << '\n';
    return output.str();
}

std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact)
{
    const auto raw_text = input_artifact.payload.value("raw_text", std::string{});
    return run_text_flow(raw_text);
}

} // namespace arcs::core
