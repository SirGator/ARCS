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
#include "interpretation/worker_client.hpp"
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
#include "store/commit.hpp"
#include "store/store_memory.hpp"
#include "schema/schema_loader.hpp"
#include "schema/schema_registry.hpp"
#include "reducer/mock_time_source.hpp"
#include "reducer/permission_reducer.hpp"
#include "verification/authority_verifier.hpp"
#include "verification/verifier.hpp"

namespace arcs::core {

namespace {

using ArtifactVersion = arcs::artifact::ArtifactVersion;
using CommitBundle = arcs::store::commit::CommitBundle;
using PendingVersion = arcs::store::commit::PendingVersion;
using Event = arcs::event::Event;
using EventRef = arcs::event::EventRef;

std::string utc_now();
std::string utc_at(std::chrono::system_clock::time_point time_point);
std::string utc_after_hours(int hours);

std::filesystem::path artifacts_base_dir()
{
    if (const char* env = std::getenv("ARCS_ARTIFACT_DIR"); env != nullptr && *env != '\0') {
        return std::filesystem::path(env);
    }

    return std::filesystem::path("artifacts");
}

const arcs::schema::SchemaRegistry& payload_schema_registry()
{
    static const auto registry = [] {
        arcs::schema::SchemaRegistry registry;
        const auto schemas_dir = std::filesystem::path(__FILE__).parent_path()
            .parent_path().parent_path().parent_path()
            / "schemas" / "v1";

        for (const auto& entry : std::filesystem::directory_iterator(schemas_dir)) {
            if (!entry.is_regular_file() || entry.path().extension() != ".json") {
                continue;
            }

            const auto schema_entry = arcs::schema::SchemaLoader::load_from_file(entry.path());
            if (!schema_entry.has_value() || !registry.register_schema(*schema_entry)) {
                throw std::runtime_error("payload schema registry could not be loaded");
            }
        }

        return registry;
    }();

    return registry;
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

std::optional<nlohmann::json> interpret_free_text(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config,
    SystemLogger& logger,
    std::ostringstream& output)
{
    if (interpretation_config == nullptr) {
        return std::nullopt;
    }

    if (!interpretation_config->interpret_api_url.has_value()) {
        return std::nullopt;
    }

    arcs::interpretation::WorkerInterpretationClient client(*interpretation_config);
    const arcs::interpretation::InterpretationRequest input_request{
        .request_id = "req_free_text",
        .raw_input = input,
        .schema_id = "arcs.interpretation_proposal.v1",
        .schema = nlohmann::json::parse(R"({
          "$id": "arcs.interpretation_proposal.v1",
          "$schema": "https://json-schema.org/draft/2020-12/schema",
          "title": "ARCS Interpretation Proposal",
          "type": "object",
          "required": ["status", "intent", "confidence", "slots", "missing_required_fields", "next_step"],
          "properties": {
            "status": {
              "type": "string",
              "enum": ["ok", "blocked", "needs_clarification"]
            },
            "intent": {
              "type": "object",
              "required": ["name", "category", "description"],
              "properties": {
                "name": { "type": "string" },
                "category": { "type": "string" },
                "description": { "type": "string" }
              },
              "additionalProperties": true
            },
            "confidence": {
              "type": "number",
              "minimum": 0.0,
              "maximum": 1.0
            },
            "slots": { "type": "object" },
            "missing_required_fields": { "type": "array", "items": { "type": "string" } },
            "next_step": {
              "type": "string",
              "enum": ["execute", "ask", "block"]
            }
          },
          "additionalProperties": true
        })"),
        .context = nlohmann::json{
            {"timezone", "Europe/Berlin"},
            {"language", "de"},
            {"current_time", utc_now()},
        },
        .prompt_config = nlohmann::json{
            {"mode", "strict_json"},
            {"temperature", 0.0},
        },
    };

    const auto input_response = client.interpret(input_request);
    if (!input_response.ok) {
        const auto error = input_response.error.value_or("unknown error");
        logger.fail("interpret", error);
        output << "step: interpret -> FAIL | " << error << '\n';
        return std::nullopt;
    }

    output << "step: interpret -> OK\n";

    if (!input_response.request_id.empty()) {
        output << "interpretation request_id: " << input_response.request_id << '\n';
    }
    if (!input_response.schema_id.empty()) {
        output << "interpretation schema_id: " << input_response.schema_id << '\n';
    }

    if (!input_response.payload.is_object()) {
        logger.fail("interpretation artifact", "payload is not an object");
        output << "step: interpretation artifact -> FAIL | payload is not an object\n";
        return std::nullopt;
    }

    output << "step: interpretation artifact -> OK\n";
    return input_response.payload;
}

ArtifactVersion make_interpretation_proposal_artifact(
    const ArtifactVersion& ingress_event,
    const nlohmann::json& interpretation_payload)
{
    ArtifactVersion artifact = arcs::artifact::factory::make_base_artifact(
        "interpretation_proposal",
        "arcs.interpretation_proposal.v1",
        ingress_event.stream_key,
        "system",
        "interpretation_worker",
        "api",
        "interpret",
        "low",
        "external",
        utc_now());

    artifact.payload = interpretation_payload;
    artifact.provenance.parents = {ingress_event.artifact_id};
    artifact.provenance.rules_applied = {"external_interpretation"};
    artifact.provenance.transform = "interpret_free_text";
    return artifact;
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
    const std::string& trust_source_class);

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

ParsedInput parse_input(const std::string& input, const bool enable_demo_controls)
{
    if (!enable_demo_controls) {
        return ParsedInput{};
    }

    return parse_input(input);
}

std::string utc_now()
{
    return utc_at(std::chrono::system_clock::now());
}

std::string utc_at(const std::chrono::system_clock::time_point time_point)
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(time_point);
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

std::string utc_after_hours(const int hours)
{
    return utc_at(std::chrono::system_clock::now() + std::chrono::hours(hours));
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

void append_bundle(CommitBundle& destination, const CommitBundle& source)
{
    destination.versions.insert(
        destination.versions.end(),
        source.versions.begin(),
        source.versions.end());
    destination.events.insert(
        destination.events.end(),
        source.events.begin(),
        source.events.end());
}

std::string first_blocker_or(const arcs::verification::VerificationReportData& report, const std::string& fallback)
{
    if (!report.blockers.empty()) {
        return report.blockers.front();
    }

    return fallback;
}

std::vector<std::string> verifier_rule_names(const ArtifactVersion& policy)
{
    std::vector<std::string> names;

    if (!policy.payload.is_object() || !policy.payload.contains("verifier_rules")) {
        return names;
    }

    const auto& verifier_rules = policy.payload.at("verifier_rules");
    if (!verifier_rules.is_object()) {
        return names;
    }

    const auto append_rule_names = [&names, &verifier_rules](const char* key) {
        if (!verifier_rules.contains(key) || !verifier_rules.at(key).is_array()) {
            return;
        }

        for (const auto& entry : verifier_rules.at(key)) {
            if (entry.is_string()) {
                names.push_back(entry.get<std::string>());
            }
        }
    };

    append_rule_names("hard_checks");
    append_rule_names("soft_checks");
    return names;
}

void add_policy_driven_verifiers(arcs::verification::VerificationEngine& engine,
                                 std::vector<std::string>& deferred_checks,
                                 const ArtifactVersion& policy)
{
    for (const auto& check_name : verifier_rule_names(policy)) {
        if (check_name == "permission") {
            engine.add_verifier(std::make_shared<arcs::verification::PermissionVerifier>());
            continue;
        }

        if (check_name == "scope") {
            engine.add_verifier(std::make_shared<arcs::verification::ScopeVerifier>());
            continue;
        }

        if (check_name == "approval" || check_name == "policy_drift") {
            deferred_checks.push_back(check_name);
            continue;
        }

        deferred_checks.push_back(check_name);
    }
}

void append_deferred_policy_checks(
    arcs::verification::VerificationReportData& report,
    const std::vector<std::string>& deferred_checks,
    bool approval_ok,
    bool policy_drift)
{
    for (const auto& check_name : deferred_checks) {
        if (check_name == "approval") {
            report.checks.push_back(arcs::verification::VerificationCheck{
                .name = "approval",
                .status = approval_ok ? arcs::verification::CheckStatus::Pass
                                      : arcs::verification::CheckStatus::Fail,
                .detail = approval_ok ? "approval requested" : "missing approval",
            });
            continue;
        }

        if (check_name == "policy_drift") {
            report.checks.push_back(arcs::verification::VerificationCheck{
                .name = "policy_drift",
                .status = policy_drift ? arcs::verification::CheckStatus::Fail
                                       : arcs::verification::CheckStatus::Pass,
                .detail = policy_drift ? "option.policy_ref does not match current policy head"
                                       : "policy head matches option binding",
            });
            continue;
        }

        report.checks.push_back(arcs::verification::VerificationCheck{
            .name = check_name,
            .status = arcs::verification::CheckStatus::Unknown,
            .detail = "policy requested unsupported verifier in core flow",
        });
    }
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

ArtifactVersion make_permission_grant_artifact(
    const ArtifactVersion& task,
    const std::string& principal,
    const std::string& capability,
    const std::string& expires_at)
{
    ArtifactVersion artifact = make_artifact(
        "permission_grant",
        "arcs.permission_grant.v1",
        task.stream_key,
        "system",
        "kernel",
        "internal",
        "permission_grant",
        "high",
        "system");

    artifact.payload = nlohmann::json{
        {"principal", principal},
        {"capability", capability},
        {"scope", task.stream_key},
        {"expires_at", expires_at},
    };
    artifact.provenance.parents = {task.artifact_id};
    artifact.provenance.rules_applied = {"permission_grant_demo"};
    artifact.provenance.transform = "grant_permission";
    return artifact;
}

ArtifactVersion make_approval_request_artifact(
    const ArtifactVersion& option,
    const ArtifactVersion& policy,
    const ArtifactVersion& verification_report,
    const ArtifactVersion& action,
    const std::string& requested_at,
    const std::string& store_head_at_request,
    const std::string& risk_summary)
{
    ArtifactVersion artifact = make_artifact(
        "approval_request",
        "arcs.approval_request.v1",
        option.stream_key,
        "system",
        "kernel",
        "internal",
        "approval_request",
        "high",
        "system");

    artifact.payload = nlohmann::json{
        {"target_option", {{"artifact_id", option.artifact_id}, {"version_id", option.version_id}}},
        {"policy_ref", {{"artifact_id", policy.artifact_id}, {"version_id", policy.version_id}}},
        {"verification_ref", {{"artifact_id", verification_report.artifact_id}, {"version_id", verification_report.version_id}}},
        {"action_ref", {{"artifact_id", action.artifact_id}, {"version_id", action.version_id}}},
        {"requested_scope", option.stream_key},
        {"requested_at", requested_at},
        {"store_head_at_request", store_head_at_request},
        {"risk_summary", risk_summary},
    };
    artifact.provenance.parents = {option.artifact_id, policy.artifact_id, verification_report.artifact_id, action.artifact_id};
    artifact.provenance.rules_applied = {"approval_request_gate"};
    artifact.provenance.transform = "request_approval";
    return artifact;
}

std::string make_risk_summary(const ArtifactVersion& option, const ArtifactVersion& action)
{
    const auto safety_level = option.payload.value("safety_level", std::string{"unknown"});
    const auto action_type = action.payload.value("type", std::string{"unknown"});
    return "safety_level=" + safety_level + "; action_type=" + action_type;
}

ArtifactVersion make_named_verification_report_artifact(
    const ArtifactVersion& target,
    const arcs::verification::VerificationReportData& report,
    const std::string& artifact_id,
    const std::string& version_id)
{
    return arcs::verification::make_verification_report_artifact(
        target,
        report,
        arcs::artifact::ActorRef{.actor_type = "system", .id = "kernel"},
        arcs::artifact::SourceRef{.kind = "internal", .ref = "verification"},
        arcs::artifact::TrustInfo{.level = "high", .source_class = "system"},
        artifact_id,
        version_id,
        target.stream_key,
        utc_now());
}

} // namespace

std::string run_text_flow(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config,
    const FlowOptions& options)
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

    std::optional<ArtifactVersion> interpretation_artifact;
    std::optional<ArtifactVersion> interpretation_report_artifact;
    std::optional<ParsedInput> parsed_input;

    if (free_text) {
        logger.ok("parse input", "free text routed through ingress and external interpretation artifact");
        output << logger.format();

        const auto interpreted_payload = interpret_free_text(input, interpretation_config, logger, output);
        if (!interpreted_payload.has_value()) {
            output << "decision: blocked\n";
            output << "reason: free text interpretation unavailable\n";
            CommitBundle bundle{};
            add_version(bundle, ingress_event);
            const auto final_output = finalize_output(output, run_dir);
            persist_run_artifacts(run_dir, bundle, quarantine, input, final_output);
            return final_output;
        }

        interpretation_artifact = make_interpretation_proposal_artifact(ingress_event, *interpreted_payload);
        logger.ok(
            "interpretation_proposal",
            "artifact created | artifact_id=" + interpretation_artifact->artifact_id +
            " version=" + interpretation_artifact->version_id);

        arcs::verification::VerificationEngine interpretation_verification_engine;
        interpretation_verification_engine.add_verifier(std::make_shared<arcs::verification::SchemaVerifier>());

        arcs::verification::VerificationContext interpretation_verification_context{};
        interpretation_verification_context.schema_registry = &payload_schema_registry();

        const auto interpretation_report = interpretation_verification_engine.run_all(
            *interpretation_artifact,
            interpretation_verification_context);
        interpretation_report_artifact = make_named_verification_report_artifact(
            *interpretation_artifact,
            interpretation_report,
            "a_interpretation_verification_report",
            "v_interpretation_verification_report");

        if (interpretation_report.status == arcs::verification::CheckStatus::Pass) {
            logger.ok("interpretation_verification_report", "pass | checks=" + std::to_string(interpretation_report.checks.size()));
        } else {
            logger.fail("interpretation_verification_report", arcs::verification::to_string(interpretation_report.status));
            output << logger.format();
            output << "decision: blocked\n";
            output << "reason: interpretation verification blocked\n";
            CommitBundle bundle{};
            add_version(bundle, ingress_event);
            add_version(bundle, *interpretation_artifact);
            add_version(bundle, *interpretation_report_artifact);
            const auto final_output = finalize_output(output, run_dir);
            persist_run_artifacts(run_dir, bundle, quarantine, input, final_output);
            return final_output;
        }

        output << "interpretation: external worker accepted\n";
        parsed_input = ParsedInput{};
    } else {
        parsed_input = parse_input(input, options.enable_demo_controls);
    }

    logger.ok(
        "parse input",
        std::string(options.enable_demo_controls ? "demo control parsed | approval=" :
                        "demo control disabled | approval=") +
        (parsed_input->approval_yes ? "yes" : "no") +
        " permission=" + (parsed_input->permission_yes ? "yes" : "no") +
        " policy_drift=" + (parsed_input->policy_drift ? "yes" : "no"));

    ArtifactVersion task_artifact = make_artifact(
        "task",
        "arcs.task.v1",
        ingress_event.stream_key,
        "system",
        "kernel",
        "internal",
        "input",
        "high",
        "system");
    task_artifact.payload = nlohmann::json{
        {"title", "Input task"},
        {"description", input},
        {"approval", parsed_input->approval_yes},
        {"permission", parsed_input->permission_yes},
        {"policy_drift", parsed_input->policy_drift},
    };
    task_artifact.provenance.parents = {ingress_event.artifact_id};
    task_artifact.provenance.rules_applied = {"task_from_input"};
    task_artifact.provenance.transform = "derive_task_from_input";

    logger.ok(
        "task",
        "artifact created | task_id=" + task_artifact.artifact_id +
        " version=" + task_artifact.version_id);

    const auto& parsed = *parsed_input;
    const bool approval_ok = parsed.approval_yes;
    const bool policy_drift = parsed.policy_drift;
    constexpr std::string_view kExecutionPrincipal = "user:cli";

    ArtifactVersion permission_grant_artifact{};
    if (parsed.permission_yes) {
        permission_grant_artifact = make_permission_grant_artifact(
            task_artifact,
            std::string{kExecutionPrincipal},
            "exec:report_emit",
            utc_after_hours(1));
        logger.ok(
            "permission_grant",
            "artifact created | principal=" + std::string{kExecutionPrincipal} +
            " capability=exec:report_emit");
    }

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
            {"hard_checks", {"permission", "scope", "approval", "policy_drift"}},
            {"soft_checks", nlohmann::json::array()},
        }},
        {"approval_required_for", {"exec:report_emit"}},
    };
    policy_current.provenance.rules_applied = {"policy_bootstrap"};
    policy_current.provenance.transform = "policy_current";

    ArtifactVersion policy_previous = policy_current;
    policy_previous.version_id = "v_policy_001";
    policy_previous.payload["verifier_rules"]["hard_checks"] = {"permission", "scope", "approval"};
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
        {"human_summary", "Emit a JSON report summarizing the interpreted input."},
        {"safety_level", "low"},
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

    logger.ok("option", "artifact created | policy_ref=" + policy_ref.artifact_id + ":" + policy_ref.version_id + " action=report_emit");

    if (approval_ok) {
        logger.ok("check approval", "approval=yes");
    } else {
        logger.fail("check approval", "approval missing or not yes");
    }

    if (!permission_grant_artifact.artifact_id.empty()) {
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
    std::vector<std::string> deferred_policy_checks;
    add_policy_driven_verifiers(verification_engine, deferred_policy_checks, policy_current);

    std::vector<ArtifactVersion> permission_artifacts;
    if (!permission_grant_artifact.artifact_id.empty()) {
        permission_artifacts.push_back(permission_grant_artifact);
    }

    arcs::reducer::MockTimeSource permission_time_source(utc_now());
    arcs::reducer::PermissionReducer permission_reducer(std::string{kExecutionPrincipal}, permission_time_source);
    auto effective_permissions = permission_reducer.reduce(permission_artifacts);

    arcs::verification::VerificationContext verification_context{};
    verification_context.permissions = effective_permissions;
    verification_context.permissions.scopes.push_back(task_artifact.stream_key);

    auto report = verification_engine.run_all(option, verification_context);
    append_deferred_policy_checks(report, deferred_policy_checks, approval_ok, policy_drift);

    report = arcs::verification::make_verification_report(option, std::move(report.checks));

    auto report_artifact = make_named_verification_report_artifact(
        option,
        report,
        "a_option_verification_report",
        "v_option_verification_report");

    if (report.status == arcs::verification::CheckStatus::Pass) {
        logger.ok("verification_report", "pass | checks=" + std::to_string(report.checks.size()));
    } else {
        logger.fail("verification_report", arcs::verification::to_string(report.status));
    }

    ArtifactVersion approval_request_artifact{};
    ArtifactVersion approval_artifact{};
    ArtifactVersion action_artifact{};
    ArtifactVersion action_report_artifact{};
    ArtifactVersion execution_result_artifact{};
    std::string reason;
    std::string decision_status = "blocked";
    arcs::store::StoreMemory store;
    CommitBundle persisted_bundle{};
    persisted_bundle.versions.reserve(12);

    if (report.status != arcs::verification::CheckStatus::Pass) {
        reason = first_blocker_or(report, "verification blocked");
        logger.fail("decision", reason);
    } else {
        arcs::execution::ActionMaterializer materializer;
        auto actions = materializer.materialize(option, policy_current);
        if (actions.empty()) {
            reason = "no action materialized";
            logger.fail("decision", reason);
        } else {
            action_artifact = actions.front();
            const auto risk_summary = make_risk_summary(option, action_artifact);
            logger.ok(
                "materialize action",
                "report_emit | action_id=" + action_artifact.artifact_id +
                " version=" + action_artifact.version_id);

            approval_request_artifact = make_approval_request_artifact(
                option,
                policy_current,
                report_artifact,
                action_artifact,
                utc_now(),
                report_artifact.version_id,
                risk_summary);
            logger.ok(
                "approval_request",
                "artifact created | request_id=" + approval_request_artifact.artifact_id +
                " action_candidate_ref=" + action_artifact.artifact_id + ":" + action_artifact.version_id);

            CommitBundle pre_approval_bundle{};
            pre_approval_bundle.versions.reserve(10);
            add_version(pre_approval_bundle, ingress_event);
            if (interpretation_artifact.has_value()) {
                add_version(pre_approval_bundle, *interpretation_artifact);
            }
            if (interpretation_report_artifact.has_value()) {
                add_version(pre_approval_bundle, *interpretation_report_artifact);
            }
            add_version(pre_approval_bundle, task_artifact);
            if (!permission_grant_artifact.artifact_id.empty()) {
                add_version(pre_approval_bundle, permission_grant_artifact);
            }
            add_version(pre_approval_bundle, policy_previous);
            add_version(pre_approval_bundle, policy_current);
            add_version(pre_approval_bundle, option);
            add_version(pre_approval_bundle, report_artifact);
            add_version(pre_approval_bundle, approval_request_artifact);
            add_version(pre_approval_bundle, action_artifact);
            store.commit(pre_approval_bundle);
            append_bundle(persisted_bundle, pre_approval_bundle);

            const auto store_head_at_approval = store.current_head_version_id(action_artifact.artifact_id)
                .value_or(action_artifact.version_id);

            arcs::approval::ApprovalGate approval_gate;
            arcs::approval::ApprovalPayload approval_payload{
                .target_option = {option.artifact_id, option.version_id},
                .policy_ref = {policy_current.artifact_id, policy_current.version_id},
                .verification_ref = {report_artifact.artifact_id, report_artifact.version_id},
                .request_ref = {approval_request_artifact.artifact_id, approval_request_artifact.version_id},
                .action_ref = {action_artifact.artifact_id, action_artifact.version_id},
                .decision = arcs::approval::ApprovalDecision::Approve,
                .reason = "kernel approval for report_emit",
                .actor = {"human", "user:cli"},
                .timestamp = utc_now(),
                .expires_at = utc_after_hours(1),
                .approval_scope = option.stream_key,
                .store_head_at_approval = store_head_at_approval,
                .risk_summary = risk_summary,
            };

            approval_artifact = approval_gate.submit(approval_payload);
            approval_artifact.stream_key = option.stream_key;

            logger.ok(
                "approval",
                "approval artifact created | approval_id=" + approval_artifact.artifact_id +
                " request_ref=" + approval_request_artifact.artifact_id +
                " action_ref=" + action_artifact.artifact_id + ":" + action_artifact.version_id);

            CommitBundle approval_bundle{};
            approval_bundle.versions.reserve(1);
            add_version(approval_bundle, approval_artifact);
            store.commit(approval_bundle);
            append_bundle(persisted_bundle, approval_bundle);

            arcs::verification::VerificationEngine action_verification_engine;
            action_verification_engine.add_verifier(std::make_shared<arcs::verification::SchemaVerifier>());
            action_verification_engine.add_verifier(std::make_shared<arcs::verification::ReferenceIntegrityVerifier>());
            action_verification_engine.add_verifier(std::make_shared<arcs::verification::PermissionVerifier>());
            action_verification_engine.add_verifier(std::make_shared<arcs::verification::ApprovalVerifier>());
            action_verification_engine.add_verifier(std::make_shared<arcs::verification::AuthorityVerifier>());

            arcs::verification::VerificationContext action_verification_context{};
            action_verification_context.policy = &policy_current;
            action_verification_context.permissions = verification_context.permissions;
            action_verification_context.schema_registry = &payload_schema_registry();
            action_verification_context.store = &store;
            action_verification_context.time_source = &permission_time_source;

            auto action_report = action_verification_engine.run_all(action_artifact, action_verification_context);
            action_report_artifact = make_named_verification_report_artifact(
                action_artifact,
                action_report,
                "a_action_verification_report",
                "v_action_verification_report");

            if (action_report.status == arcs::verification::CheckStatus::Pass) {
                logger.ok(
                    "action_verification_report",
                    "pass | checks=" + std::to_string(action_report.checks.size()));
            } else {
                logger.fail(
                    "action_verification_report",
                    arcs::verification::to_string(action_report.status));
            }

            CommitBundle action_report_bundle{};
            action_report_bundle.versions.reserve(1);
            add_version(action_report_bundle, action_report_artifact);
            store.commit(action_report_bundle);
            append_bundle(persisted_bundle, action_report_bundle);

            if (action_report.status != arcs::verification::CheckStatus::Pass) {
                reason = first_blocker_or(action_report, "action verification blocked");
                logger.fail("decision", reason);
                goto finalize_decision;
            }

            KernelIdempotencyStore idempotency_store;
            arcs::execution::ReportEmitExecutor executor(idempotency_store);
            arcs::execution::ExecutionContext execution_context{};
            execution_context.approval_id = approval_artifact.artifact_id;
            execution_context.verification_id = action_report_artifact.artifact_id;
            execution_context.approval_valid = true;
            execution_context.approval_expires_at = approval_artifact.payload.value("expires_at", std::string{});
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
                decision_status = "not_blocked";
                reason = options.enable_demo_controls ? "demo approval and permission granted" : "approved action executed";
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

finalize_decision:

    auto decision_artifact = make_decision_artifact(
        option,
        report,
        decision_status,
        reason,
        approval_artifact.artifact_id,
        action_artifact.artifact_id,
        execution_result_artifact.artifact_id);

    if (!action_artifact.artifact_id.empty()) {
        CommitBundle post_execution_bundle{};
        post_execution_bundle.versions.reserve(2);
        if (!execution_result_artifact.artifact_id.empty()) {
            add_version(post_execution_bundle, execution_result_artifact);
        }
        add_version(post_execution_bundle, decision_artifact);
        store.commit(post_execution_bundle);
        append_bundle(persisted_bundle, post_execution_bundle);
    } else {
        CommitBundle bundle{};
        bundle.versions.reserve(12);
        add_version(bundle, ingress_event);
        if (interpretation_artifact.has_value()) {
            add_version(bundle, *interpretation_artifact);
        }
        if (interpretation_report_artifact.has_value()) {
            add_version(bundle, *interpretation_report_artifact);
        }
        add_version(bundle, task_artifact);
        if (!permission_grant_artifact.artifact_id.empty()) {
            add_version(bundle, permission_grant_artifact);
        }
        add_version(bundle, policy_previous);
        add_version(bundle, policy_current);
        add_version(bundle, option);
        add_version(bundle, report_artifact);
        if (!approval_request_artifact.artifact_id.empty()) {
            add_version(bundle, approval_request_artifact);
        }
        if (!approval_artifact.artifact_id.empty()) {
            add_version(bundle, approval_artifact);
        }
        if (!action_report_artifact.artifact_id.empty()) {
            add_version(bundle, action_report_artifact);
        }
        add_version(bundle, decision_artifact);
        store.commit(bundle);
        append_bundle(persisted_bundle, bundle);
    }

    if (decision_status == "not_blocked") {
        output << logger.format();
        output << "decision: not blocked\n";
        output << "reason: " << reason << '\n';
        const auto final_output = finalize_output(output, run_dir);
        persist_run_artifacts(run_dir, persisted_bundle, quarantine, input, final_output);
        return final_output;
    }

    output << logger.format();
    output << "decision: blocked\n";
    output << "reason: " << reason << '\n';
    const auto final_output = finalize_output(output, run_dir);
    persist_run_artifacts(run_dir, persisted_bundle, quarantine, input, final_output);
    return final_output;
}

std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact)
{
    const auto raw_text = input_artifact.payload.value("raw_text", std::string{});
    return run_text_flow(raw_text, nullptr, {});
}

std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact, const FlowOptions& options)
{
    const auto raw_text = input_artifact.payload.value("raw_text", std::string{});
    return run_text_flow(raw_text, nullptr, options);
}

} // namespace arcs::core
