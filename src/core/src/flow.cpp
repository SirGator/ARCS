#include "core/flow.hpp"

#include <algorithm>
#include <cstdlib>
#include <cctype>
#include <chrono>
#include <ctime>
#include <filesystem>
#include <fstream>
#include <iomanip>
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
#include "event/json.hpp"
#include "core/system_logger.hpp"
#include "execution/action.hpp"
#include "materializer.hpp"
#include "execution/report_emit_executor.hpp"
#include "execution/idempotency.hpp"
#include "ingress/ingress_source.hpp"
#include "ingress/cli_ingress_source.hpp"
#include "ingress/ingress_normalizer.hpp"
#include "ingress/ingress_validator.hpp"
#include "ingress/ingress_router.hpp"
#include "ingress/quarantine.hpp"
#include "interpretation/api_interpreter.hpp"
#include "interpretation/interpretation_to_task_mapper.hpp"
#include "interpretation/rule_based_interpreter.hpp"
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

std::filesystem::path artifacts_base_dir()
{
    if (const char* env = std::getenv("ARCS_ARTIFACT_DIR"); env != nullptr && *env != '\0') {
        return std::filesystem::path(env);
    }

    return std::filesystem::path("artifacts");
}

std::string run_timestamp()
{
    const auto now = std::chrono::system_clock::now();
    const auto now_time_t = std::chrono::system_clock::to_time_t(now);
    std::tm tm{};

#if defined(_WIN32)
    localtime_s(&tm, &now_time_t);
#else
    localtime_r(&now_time_t, &tm);
#endif

    const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(
        now.time_since_epoch()) % 1000;

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%d_%H-%M-%S")
        << '-' << std::setw(3) << std::setfill('0') << millis.count();
    return out.str();
}

std::filesystem::path make_run_artifacts_dir()
{
    const auto run_dir = artifacts_base_dir() / run_timestamp();
    std::error_code ec;
    std::filesystem::create_directories(run_dir / "artifacts", ec);
    std::filesystem::create_directories(run_dir / "events", ec);
    std::filesystem::create_directories(run_dir / "quarantine", ec);
    return run_dir;
}

std::string safe_filename_component(std::string value)
{
    for (char& ch : value) {
        if (!std::isalnum(static_cast<unsigned char>(ch)) && ch != '-' && ch != '_') {
            ch = '_';
        }
    }

    if (value.empty()) {
        return "unknown";
    }

    return value;
}

void write_json_file(const std::filesystem::path& path, const nlohmann::json& value)
{
    std::ofstream out(path);
    if (!out) {
        return;
    }

    out << value.dump(2) << '\n';
}

void write_text_file(const std::filesystem::path& path, const std::string& text)
{
    std::ofstream out(path);
    if (!out) {
        return;
    }

    out << text;
    if (!text.empty() && text.back() != '\n') {
        out << '\n';
    }
}

void persist_run_artifacts(
    const std::filesystem::path& run_dir,
    const CommitBundle& bundle,
    const arcs::ingress::QuarantineStore& quarantine,
    const std::string& input,
    const std::string& output)
{
    nlohmann::json manifest;
    manifest["input"] = input;
    manifest["artifacts"] = nlohmann::json::array();
    manifest["events"] = nlohmann::json::array();
    manifest["quarantine"] = nlohmann::json::array();

    write_text_file(run_dir / "input.txt", input);
    write_text_file(run_dir / "output.txt", output);

    for (std::size_t index = 0; index < bundle.versions.size(); ++index) {
        const auto& pending = bundle.versions[index];
        const auto& version = pending.version;
        const auto filename = std::to_string(index + 1) + "_" +
            safe_filename_component(version.type) + "_" +
            safe_filename_component(version.artifact_id) + "_" +
            safe_filename_component(version.version_id) + ".json";

        write_json_file(run_dir / "artifacts" / filename, version);
        manifest["artifacts"].push_back({
            {"file", (std::filesystem::path("artifacts") / filename).string()},
            {"artifact_id", version.artifact_id},
            {"version_id", version.version_id},
            {"type", version.type},
            {"schema_id", version.schema_id},
        });
    }

    for (std::size_t index = 0; index < bundle.events.size(); ++index) {
        const auto& event = bundle.events[index];
        const auto filename = std::to_string(index + 1) + "_" +
            safe_filename_component(event.event_type) + "_" +
            safe_filename_component(event.event_id) + ".json";

        write_json_file(run_dir / "events" / filename, event);
        manifest["events"].push_back({
            {"file", (std::filesystem::path("events") / filename).string()},
            {"event_id", event.event_id},
            {"event_type", event.event_type},
            {"stream_key", event.stream_key},
        });
    }

    for (std::size_t index = 0; index < quarantine.events().size(); ++index) {
        const auto& quarantined = quarantine.events()[index];
        const auto filename = std::to_string(index + 1) + "_" +
            safe_filename_component(quarantined.artifact.type) + "_" +
            safe_filename_component(quarantined.artifact.artifact_id) + "_" +
            safe_filename_component(quarantined.artifact.version_id) + ".json";

        write_json_file(
            run_dir / "quarantine" / filename,
            nlohmann::json{
                {"artifact", quarantined.artifact},
                {"rejection_reason", quarantined.rejection_reason},
                {"rejected_at", quarantined.rejected_at},
                {"rejection_stage", quarantined.rejection_stage},
            });

        manifest["quarantine"].push_back({
            {"file", (std::filesystem::path("quarantine") / filename).string()},
            {"artifact_id", quarantined.artifact.artifact_id},
            {"version_id", quarantined.artifact.version_id},
            {"rejection_reason", quarantined.rejection_reason},
            {"rejection_stage", quarantined.rejection_stage},
        });
    }

    write_json_file(run_dir / "manifest.json", manifest);
}

std::string finalize_output(std::ostringstream& output, const std::filesystem::path& run_dir)
{
    output << "artifacts: " << run_dir.string() << '\n';
    return output.str();
}

std::unique_ptr<arcs::interpretation::IInputInterpreter> make_input_interpreter()
{
    const auto config = arcs::interpretation::api_interpreter_config_from_environment();
    if (!config.endpoint_url.empty()) {
        return std::make_unique<arcs::interpretation::ApiInterpreter>(std::move(config));
    }

    return std::make_unique<arcs::interpretation::RuleBasedInterpreter>();
}

// ---- Ingress Pipeline ----

struct IngressResult {
    bool success{false};
    ArtifactVersion ingress_artifact;
    ingress::RouteAction route_action{ingress::RouteAction::Quarantine};
    std::string rejection_reason;
    std::string rejection_stage;
};

IngressResult run_ingress_pipeline(
    const std::string& raw_input,
    ingress::QuarantineStore& quarantine)
{
    IngressResult result;

    // 1. Source: create IngressEvent from raw string
    std::istringstream stream(raw_input);
    ingress::CliIngressSource source(stream);
    if (!source.has_more()) {
        result.rejection_reason = "no input";
        result.rejection_stage = "source";
        return result;
    }

    auto raw_event = source.emit();

    // 2. Normalize: IngressEvent → ingress_event artifact
    ingress::DefaultIngressNormalizer normalizer("session:cli");
    auto normalized = normalizer.normalize(raw_event);

    if (normalized.status != ingress::NormalizerStatus::Ok) {
        result.rejection_reason = normalized.rejection_reason;
        result.rejection_stage = "normalize";
        ingress::QuarantinedEvent q;
        q.artifact = normalized.artifact;
        q.rejection_reason = normalized.rejection_reason;
        q.rejected_at = normalized.artifact.created_at;
        q.rejection_stage = "normalize";
        quarantine.store(std::move(q));
        return result;
    }

    // 3. Validate: check required fields
    ingress::MinimalIngressValidator validator;
    auto validation = validator.validate(normalized.artifact);

    if (validation.status != ingress::ValidationStatus::Pass) {
        result.rejection_reason = validation.reason;
        result.rejection_stage = "validate";
        ingress::QuarantinedEvent q;
        q.artifact = normalized.artifact;
        q.rejection_reason = validation.reason;
        q.rejected_at = normalized.artifact.created_at;
        q.rejection_stage = "validate";
        quarantine.store(std::move(q));
        return result;
    }

    // 4. Route: decide what handler is responsible
    ingress::DefaultIngressRouter router;
    router.add_handler(ingress::DefaultIngressRouter::Handler{
        .name = "nlu_task_extractor",
        .source_kinds = {"chat"},
        .intent_keywords = {},
        .action = ingress::RouteAction::ExtractToTask,
    });
    router.add_handler(ingress::DefaultIngressRouter::Handler{
        .name = "passthrough",
        .source_kinds = {"internal"},
        .intent_keywords = {},
        .action = ingress::RouteAction::PassThrough,
    });

    auto route = router.route(normalized.artifact);

    result.success = true;
    result.ingress_artifact = std::move(normalized.artifact);
    result.route_action = route.action;

    return result;
}

// ---- Rest of flow ----

struct ParsedInput {
    bool approval_yes{false};
    bool permission_yes{false};
    bool policy_drift{false};
};

ArtifactVersion make_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& actor_type,
    const std::string& actor_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class);

ArtifactVersion make_interpretation_artifact(
    const std::string& stream_key,
    const ArtifactVersion& parent,
    const arcs::interpretation::InterpretationProposal& proposal)
{
    ArtifactVersion artifact = make_artifact(
        "interpretation_proposal",
        "arcs.interpretation_proposal.v1",
        stream_key,
        "system",
        "interpretation-mapper",
        "internal",
        "interpretation",
        "low",
        "rule");

    artifact.payload = nlohmann::json::object();
    artifact.payload["status"] = arcs::interpretation::to_string(proposal.status);
    artifact.payload["intent"] = proposal.intent;
    artifact.payload["target"] = proposal.target;
    artifact.payload["format"] = proposal.format;
    artifact.payload["confidence"] = proposal.confidence;
    artifact.payload["raw_text"] = proposal.raw_text;
    artifact.payload["reason"] = proposal.reason;

    artifact.payload["entities"] = nlohmann::json::array();
    for (const auto& entity : proposal.entities) {
        artifact.payload["entities"].push_back({
            {"name", entity.name},
            {"value", entity.value},
        });
    }
    artifact.provenance.parents = {parent.artifact_id};
    artifact.provenance.rules_applied = {"interpretation_parse"};
    artifact.provenance.transform = "parse_interpretation";
    return artifact;
}

ArtifactVersion make_task_artifact_from_interpretation(
    const std::string& stream_key,
    const ArtifactVersion& parent,
    const arcs::interpretation::InterpretationProposal& proposal,
    const std::optional<arcs::interpretation::TaskDraft>& task_draft)
{
    const auto draft = task_draft.has_value()
        ? *task_draft
        : arcs::interpretation::TaskDraft{
            .title = "Unknown",
            .intent = proposal.intent,
            .target = proposal.target.empty() ? proposal.raw_text : proposal.target,
            .format = proposal.format,
            .source_text = proposal.raw_text,
        };

    ArtifactVersion artifact = make_artifact(
        "task",
        "arcs.task.v1",
        stream_key,
        "system",
        "interpretation-mapper",
        "internal",
        "interpretation",
        "low",
        "rule");

    artifact.payload = nlohmann::json{
        {"title", draft.title},
        {"description", draft.source_text},
    };

    if (!draft.target.empty()) {
        artifact.payload["scope"] = draft.target;
    }

    artifact.provenance.parents = {parent.artifact_id};
    artifact.provenance.rules_applied = task_draft.has_value()
        ? std::vector<std::string>{"interpretation_to_task_mapper", "task_from_interpretation"}
        : std::vector<std::string>{"interpretation_fallback", "task_from_interpretation"};
    artifact.provenance.transform = "derive_task_from_interpretation";
    return artifact;
}

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
    const auto run_dir = make_run_artifacts_dir();
    output << "input: " << input << '\n';
    logger.ok("input received", input.empty() ? "empty" : "text present | bytes=" + std::to_string(input.size()));

    if (input.empty()) {
        logger.fail("parse input", "empty input");
        output << logger.format();
        output << "decision: blocked\n";
        output << "reason: empty input\n";
        const auto final_output = finalize_output(output, run_dir);
        persist_run_artifacts(run_dir, CommitBundle{}, ingress::QuarantineStore{}, input, final_output);
        return final_output;
    }

    const auto values = parse_key_values(input);
    const bool free_text = values.empty();

    std::optional<ParsedInput> parsed_input;
    if (!free_text) {
        parsed_input = parse_input(input);
    }

    if (free_text) {
        logger.ok("parse input", "free text routed to interpretation | stream_key=session:cli");
    } else {
        logger.ok(
            "parse input",
            std::string("key=value pairs parsed | approval=") + (parsed_input->approval_yes ? "yes" : "no") +
            " permission=" + (parsed_input->permission_yes ? "yes" : "no") +
            " policy_drift=" + (parsed_input->policy_drift ? "yes" : "no"));
    }

    const auto interpretation = make_input_interpreter();
    const auto interpretation_proposal = interpretation->interpret(input);
    arcs::interpretation::InterpretationToTaskMapper task_mapper;
    const auto task_draft = task_mapper.map(interpretation_proposal);

    // --- Ingress Pipeline ---
    ingress::QuarantineStore quarantine;
    auto ingress_result = run_ingress_pipeline(input, quarantine);

    if (!ingress_result.success) {
        logger.fail("ingress", ingress_result.rejection_reason + " (stage: " + ingress_result.rejection_stage + ")");
        output << logger.format();
        output << "decision: blocked\n";
        output << "reason: ingress rejected: " << ingress_result.rejection_reason << '\n';
        const auto final_output = finalize_output(output, run_dir);
        persist_run_artifacts(run_dir, CommitBundle{}, quarantine, input, final_output);
        return final_output;
    }

    ArtifactVersion ingress_event = ingress_result.ingress_artifact;
    logger.ok(
        "ingress_event",
        "artifact created | stream_key=" + ingress_event.stream_key +
        " source=" + ingress_event.source.kind + "/" + ingress_event.source.ref);
    const auto interpretation_artifact = make_interpretation_artifact(
        ingress_event.stream_key,
        ingress_event,
        interpretation_proposal);

    ArtifactVersion task_artifact = make_task_artifact_from_interpretation(
        ingress_event.stream_key,
        ingress_event,
        interpretation_proposal,
        task_draft);

    if (free_text) {
        logger.ok(
            "interpretation_proposal",
            std::string("status=") + arcs::interpretation::to_string(interpretation_proposal.status) +
            " | intent=" + (interpretation_proposal.intent.empty() ? "unknown" : interpretation_proposal.intent) +
            " confidence=" + std::to_string(interpretation_proposal.confidence) +
            " reason=" + (interpretation_proposal.reason.empty() ? "rule based interpretation" : interpretation_proposal.reason));

        arcs::store::StoreMemory store;
        CommitBundle bundle{};
        bundle.versions.reserve(task_draft.has_value() ? 3 : 2);
        add_version(bundle, ingress_event);
        add_version(bundle, interpretation_artifact);

        if (task_draft.has_value()) {
            add_version(bundle, task_artifact);

            logger.ok(
                "task",
                "artifact created | task_id=" + task_artifact.artifact_id +
                " version=" + task_artifact.version_id);

            logger.ok(
                "routing",
                "task-only ingestion | stream_key=" + task_artifact.stream_key);
        } else {
            logger.ok("task", "skipped | no safe task mapping");
            logger.ok(
                "routing",
                "interpretation-only ingestion | stream_key=" + ingress_event.stream_key);
        }

        store.commit(bundle);

        output << logger.format();
        output << "decision: ingested\n";

        if (task_draft.has_value()) {
            output << "reason: interpretation intent "
                   << (interpretation_proposal.intent.empty() ? "unknown" : interpretation_proposal.intent)
                   << '\n';
        } else {
            output << "reason: interpretation unknown; no task created\n";
        }

        const auto final_output = finalize_output(output, run_dir);
        persist_run_artifacts(run_dir, bundle, quarantine, input, final_output);
        return final_output;
    }

    const auto& parsed = *parsed_input;
    const bool approval_ok = parsed.approval_yes;
    const bool permission_ok = parsed.permission_yes;
    const bool policy_drift = parsed.policy_drift;

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

        logger.ok("task", "artifact created | task_id=" + task_artifact.artifact_id + " version=" + task_artifact.version_id);
        logger.ok(
            "option",
            "artifact created | policy_ref=" + policy_ref.artifact_id + ":" + policy_ref.version_id +
            " action=report_emit");

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
        logger.ok("verification_report", "pass | checks=" + std::to_string(report.checks.size()));
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

        logger.ok(
            "approval",
            "approval artifact created | approval_id=" + approval_artifact.artifact_id +
            " policy_ref=" + policy_current.artifact_id + ":" + policy_current.version_id);

        arcs::execution::ActionMaterializer materializer;
        auto actions = materializer.materialize(option, policy_current);
        if (actions.empty()) {
            reason = "no action materialized";
            logger.fail("decision", reason);
        } else {
            action_artifact = actions.front();
            logger.ok(
                "materialize action",
                "report_emit | action_id=" + action_artifact.artifact_id +
                " version=" + action_artifact.version_id);

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
                logger.ok(
                    "execute action",
                    "report_emit success | action_id=" + execution_action.artifact_id +
                    " exit_code=" + std::to_string(execution_result.exit_code));
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

    CommitBundle bundle{};
    bundle.versions.reserve(12);
    add_version(bundle, ingress_event);
    add_version(bundle, interpretation_artifact);
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

    arcs::store::StoreMemory store;
    store.commit(bundle);

    if (decision_status == "not blocked") {
        output << logger.format();
        output << "decision: not blocked\n";
        output << "reason: " << reason << '\n';
        const auto final_output = finalize_output(output, run_dir);
        persist_run_artifacts(run_dir, bundle, quarantine, input, final_output);
        return final_output;
    }

    output << logger.format();
    output << "decision: blocked\n";
    output << "reason: " << reason << '\n';
    const auto final_output = finalize_output(output, run_dir);
    persist_run_artifacts(run_dir, bundle, quarantine, input, final_output);
    return final_output;
}

std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact)
{
    const auto raw_text = input_artifact.payload.value("raw_text", std::string{});
    return run_text_flow(raw_text);
}

} // namespace arcs::core
