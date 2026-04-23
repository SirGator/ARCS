#include <filesystem>
#include <fstream>
#include <memory>
#include <iostream>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "approval.hpp"
#include "event/event.hpp"
#include "materializer.hpp"
#include "reducer/approval_state_reducer.hpp"
#include "reducer/task_state_reducer.hpp"
#include "schema/schema_registry.hpp"
#include "schema/validator.hpp"
#include "store/store_memory.hpp"
#include "verification/verifier.hpp"

namespace {

void require(bool condition, const std::string& message)
{
    if (!condition)
    {
        throw std::runtime_error(message);
    }
}

bool load_json_file(const std::filesystem::path& path, nlohmann::json& out)
{
    std::ifstream file(path);
    if (!file.is_open())
    {
        return false;
    }

    try
    {
        file >> out;
        return true;
    }
    catch (...)
    {
        return false;
    }
}

void print_result(const arcs::schema::ValidationResult& result)
{
    std::cout << (result.valid ? "VALID" : "INVALID") << '\n';
    for (const auto& error : result.errors)
    {
        std::cout << "  " << error.path << " -> " << error.message << '\n';
    }
}

template<typename Fn>
void run_named_smoke_test(const std::string& name, Fn&& fn, std::size_t& passed)
{
    fn();
    ++passed;
    std::cout << "[OK] " << name << '\n';
}

void run_schema_registry_smoke_test()
{
    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = "arcs.test.schema.v1";
    entry.document = {{"$id", entry.id}, {"type", "object"}};
    entry.source_path = "schemas/v1/test.schema.json";

    require(registry.register_schema(entry), "schema registration should succeed");
    require(!registry.register_schema(entry), "duplicate schema registration should fail");
    require(registry.find_schema(entry.id) != nullptr, "registered schema should be findable");
}

void run_schema_registry_size_smoke_test()
{
    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = "arcs.test.schema.v2";
    entry.document = {{"$id", entry.id}, {"type", "object"}};
    entry.source_path = "schemas/v1/test.schema.json";

    require(registry.register_schema(entry), "schema registration should succeed");
    require(registry.has_schema(entry.id), "schema registry should report the schema as present");
    require(registry.size() == 1, "schema registry should contain exactly one schema");
}

void run_store_smoke_test()
{
    arcs::store::StoreMemory store;

    arcs::artifact::ArtifactVersion version;
    version.artifact_id = "a_demo";
    version.version_id = "v_demo";
    version.type = "task";
    version.stream_key = "task_id:t_demo";

    arcs::event::Event event;
    event.event_id = "e_demo";
    event.event_type = "head_advanced";
    event.stream_key = "task_id:t_demo";
    event.refs.push_back({"a_demo", "v_demo", "target"});

    arcs::store::commit::CommitBundle bundle;
    bundle.versions.push_back({version, std::nullopt});
    bundle.events.push_back(event);

    store.commit(bundle);

    const auto head = store.get("a_demo");
    require(head.version_id == "v_demo", "store should expose committed head");
}

void run_store_duplicate_version_reject_smoke_test()
{
    arcs::store::StoreMemory store;

    arcs::artifact::ArtifactVersion version;
    version.artifact_id = "a_demo";
    version.version_id = "v_demo";
    version.type = "task";
    version.stream_key = "task_id:t_demo";

    arcs::store::commit::CommitBundle bundle;
    bundle.versions.push_back({version, std::nullopt});
    bundle.versions.push_back({version, std::nullopt});
    arcs::event::Event event;
    event.event_id = "e_demo";
    event.event_type = "head_advanced";
    event.stream_key = "task_id:t_demo";
    event.refs.push_back({"a_demo", "v_demo", "target"});
    bundle.events.push_back(event);

    bool rejected = false;
    try
    {
        store.commit(bundle);
    }
    catch (const arcs::store::CommitRejectedError&)
    {
        rejected = true;
    }

    require(rejected, "store should reject duplicate version ids inside a bundle");
}

void run_store_wrong_head_reject_smoke_test()
{
    arcs::store::StoreMemory store;

    arcs::artifact::ArtifactVersion version;
    version.artifact_id = "a_demo";
    version.version_id = "v_demo";
    version.type = "task";
    version.stream_key = "task_id:t_demo";

    arcs::store::commit::CommitBundle bundle;
    bundle.versions.push_back({version, std::nullopt});
    arcs::event::Event event;
    event.event_id = "e_demo";
    event.event_type = "head_advanced";
    event.stream_key = "task_id:t_demo";
    event.refs.push_back({"a_demo_wrong", "v_demo", "target"});
    bundle.events.push_back(event);

    bool rejected = false;
    try
    {
        store.commit(bundle);
    }
    catch (const arcs::store::CommitRejectedError&)
    {
        rejected = true;
    }

    require(rejected, "store should reject head_advanced events with mismatched artifact ids");
}

void run_approval_smoke_test()
{
    arcs::approval::ApprovalPayload input{};
    input.target_option = {"a_option_demo", "v_option_demo"};
    input.policy_ref = {"a_policy_demo", "v_policy_demo"};
    input.decision = arcs::approval::ApprovalDecision::Approve;
    input.reason = "smoke test";
    input.actor = {"human", "user:simon"};
    input.timestamp = "2026-04-23T10:00:00Z";
    input.expires_at = "2026-04-23T11:00:00Z";

    arcs::approval::ApprovalGate gate;
    const auto approval = gate.submit(input);

    require(approval.type == "approval", "approval gate should build approval artifacts");
    require(approval.schema_id == "arcs.approval.v1", "approval schema id should be set");
    require(approval.payload.at("decision") == "approve", "approval decision should be approve");
    require(
        approval.payload.at("policy_ref").at("version_id") == "v_policy_demo",
        "approval should carry the policy version");
}

void run_materializer_smoke_test()
{
    arcs::artifact::ArtifactVersion option;
    option.type = "option";
    option.stream_key = "task_id:t_demo";
    option.payload = {
        {"steps", {{
            {"kind", "emit_report"},
            {"params", {
                {"format", "json"},
                {"sections", {"summary"}}
            }}
        }}}
    };

    arcs::artifact::ArtifactVersion policy;
    policy.type = "policy";
    policy.payload = {
        {"capabilities", {"exec:report_emit"}},
        {"approval_required_for", {"exec:report_emit"}}
    };

    arcs::execution::ActionMaterializer materializer;
    const auto actions = materializer.materialize(option, policy);

    require(actions.size() == 1, "materializer should produce one action");
    require(actions.front().type == "action", "materialized artifact should be an action");
    require(actions.front().payload.at("type") == "report_emit", "action type should be report_emit");
    require(
        actions.front().payload.at("required_permissions")[0] == "exec:report_emit",
        "action should require report emit capability");
}

void run_task_reducer_draft_smoke_test()
{
    arcs::artifact::ArtifactVersion task;
    task.artifact_id = "a_task";
    task.version_id = "v_task";
    task.type = "task";

    arcs::reducer::TaskStateReducer reducer;
    const auto state = reducer.reduce({task});

    require(state.status == "draft", "task reducer should report draft without options");
}

void run_task_reducer_approved_smoke_test()
{
    arcs::artifact::ArtifactVersion option;
    option.artifact_id = "a_option";
    option.version_id = "v_option";
    option.type = "option";

    arcs::artifact::ArtifactVersion approval;
    approval.artifact_id = "a_approval";
    approval.version_id = "v_approval";
    approval.type = "approval";
    approval.payload = {{"decision", "approve"}};

    arcs::reducer::TaskStateReducer reducer;
    const auto state = reducer.reduce({option, approval});

    require(state.status == "approved", "task reducer should report approved when approved");
}

void run_reducer_smoke_test()
{
    arcs::artifact::ArtifactVersion option;
    option.artifact_id = "a_option";
    option.version_id = "v_option";
    option.type = "option";

    arcs::artifact::ArtifactVersion approval;
    approval.artifact_id = "a_approval";
    approval.version_id = "v_approval";
    approval.type = "approval";
    approval.payload = {{"decision", "approve"}, {"policy_ref", "p_demo"}};

    arcs::artifact::ArtifactVersion execution_result;
    execution_result.artifact_id = "a_exec";
    execution_result.version_id = "v_exec";
    execution_result.type = "execution_result";

    const std::vector<arcs::artifact::ArtifactVersion> artifacts = {
        option,
        approval,
        execution_result
    };

    arcs::reducer::TaskStateReducer task_reducer;
    const auto task_state = task_reducer.reduce(artifacts);
    require(task_state.status == "executed", "task reducer should project executed state");
    require(task_state.option_ids.size() == 1, "task reducer should collect option ids");

    arcs::reducer::ApprovalStateReducer approval_reducer;
    const auto approval_state = approval_reducer.reduce(artifacts);
    require(approval_state.valid, "approval reducer should mark approved state valid");
    require(approval_state.policy_ref == "p_demo", "approval reducer should extract policy ref");
}

void run_approval_reducer_default_smoke_test()
{
    arcs::artifact::ArtifactVersion task;
    task.artifact_id = "a_task";
    task.version_id = "v_task";
    task.type = "task";

    arcs::reducer::ApprovalStateReducer reducer;
    const auto state = reducer.reduce({task});

    require(!state.valid, "approval reducer should be invalid without approval artifacts");
}

void run_approval_reducer_revoke_smoke_test()
{
    arcs::artifact::ArtifactVersion approval;
    approval.artifact_id = "a_approval";
    approval.version_id = "v_approval";
    approval.type = "approval";
    approval.payload = {{"decision", "revoke"}, {"policy_ref", "v_policy_demo"}};

    arcs::reducer::ApprovalStateReducer reducer;
    const auto state = reducer.reduce({approval});

    require(state.decision == "revoke", "approval reducer should read revoke decision");
    require(!state.valid, "approval reducer should mark revoke as invalid");
}

void run_verification_smoke_test()
{
    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = "arcs.demo.option.v1";
    entry.document = {{"$id", entry.id}, {"type", "object"}};
    entry.source_path = "inline";

    require(registry.register_schema(entry), "verification schema registration should succeed");

    arcs::artifact::ArtifactVersion target;
    target.artifact_id = "a_option_demo";
    target.version_id = "v_option_demo";
    target.type = "option";
    target.schema_id = entry.id;
    target.schema_version = 1;
    target.stream_key = "task_id:t_demo";
    target.payload = {
        {"title", "Verification Demo"},
        {"requires_permissions", {"exec:report_emit"}},
        {"required_scopes", {"task_id:t_demo"}}
    };

    arcs::verification::VerificationEngine engine;
    engine.add_verifier(std::make_shared<arcs::verification::SchemaVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ReferenceIntegrityVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::PermissionVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ScopeVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ApprovalVerifier>());

    arcs::verification::VerificationContext context;
    context.schema_registry = &registry;
    context.permissions.capabilities = {"exec:report_emit"};
    context.permissions.scopes = {"task_id:t_demo"};

    const auto report = engine.run_all(target, context);
    require(report.status == arcs::verification::CheckStatus::Pass, "verification engine should pass smoke test");
}

void run_verification_unknown_smoke_test()
{
    arcs::artifact::ArtifactVersion target;
    target.artifact_id = "a_option_demo";
    target.version_id = "v_option_demo";
    target.type = "option";
    target.schema_id = "arcs.demo.option.v1";
    target.stream_key = "";
    target.payload = {
        {"title", "Verification Demo"},
        {"requires_permissions", {"exec:report_emit"}},
        {"required_scopes", {"task_id:t_demo"}}
    };

    arcs::verification::ScopeVerifier verifier;
    arcs::verification::VerificationContext context;

    const auto check = verifier.check(target, context);
    require(check.status == arcs::verification::CheckStatus::Unknown, "scope verifier should return unknown when stream key is missing");
}

void run_schema_validation_smoke_test()
{
    const std::filesystem::path schema_path = std::filesystem::path(ARCS_SOURCE_DIR) / "schemas/v1/task.schema.json";

    nlohmann::json schema_json;
    require(load_json_file(schema_path, schema_json), "schema should load for validation smoke test");

    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = schema_json.value("$id", "arcs.task.v1");
    entry.document = schema_json;
    entry.source_path = schema_path;
    require(registry.register_schema(entry), "schema should register for validation smoke test");

    const nlohmann::json valid_task = {
        {"title", "Phase 2 Demo"},
        {"description", "Task validation should pass"},
        {"priority", "high"}
    };

    const nlohmann::json invalid_task = {
        {"description", "Missing title and invalid priority"},
        {"priority", "urgent"}
    };

    const auto valid_result = arcs::schema::Validator::validate(valid_task, entry.id, registry);
    const auto invalid_result = arcs::schema::Validator::validate(invalid_task, entry.id, registry);

    require(valid_result.valid, "valid task should pass schema validation smoke test");
    require(!invalid_result.valid, "invalid task should fail schema validation smoke test");
}

void run_happy_path_smoke_test()
{
    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = "arcs.demo.option.v1";
    entry.document = {{"$id", entry.id}, {"type", "object"}};
    entry.source_path = "inline";
    require(registry.register_schema(entry), "happy path schema should register");

    arcs::artifact::ArtifactVersion option;
    option.artifact_id = "a_option_happy";
    option.version_id = "v_option_happy";
    option.type = "option";
    option.schema_id = entry.id;
    option.stream_key = "task_id:t_happy";
    option.payload = {
        {"steps", {{
            {"kind", "emit_report"},
            {"params", {{"format", "json"}, {"sections", {"summary"}}}}
        }}},
        {"requires_permissions", {"exec:report_emit"}},
        {"required_scopes", {"task_id:t_happy"}}
    };

    arcs::artifact::ArtifactVersion policy;
    policy.artifact_id = "a_policy_happy";
    policy.version_id = "v_policy_happy";
    policy.type = "policy";
    policy.schema_id = "arcs.policy.v1";
    policy.payload = {
        {"capabilities", {"exec:report_emit"}},
        {"approval_required_for", {"exec:report_emit"}}
    };

    arcs::approval::ApprovalPayload approval_input{};
    approval_input.target_option = {option.artifact_id, option.version_id};
    approval_input.policy_ref = {policy.artifact_id, policy.version_id};
    approval_input.decision = arcs::approval::ApprovalDecision::Approve;
    approval_input.reason = "happy path";
    approval_input.actor = {"human", "user:simon"};
    approval_input.timestamp = "2026-04-23T10:00:00Z";
    approval_input.expires_at = "2026-04-23T11:00:00Z";

    arcs::approval::ApprovalGate gate;
    const auto approval = gate.submit(approval_input);

    arcs::execution::ActionMaterializer materializer;
    const auto actions = materializer.materialize(option, policy);
    require(actions.size() == 1, "happy path should produce one action");

    arcs::verification::VerificationEngine engine;
    engine.add_verifier(std::make_shared<arcs::verification::SchemaVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ReferenceIntegrityVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::PermissionVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ScopeVerifier>());
    engine.add_verifier(std::make_shared<arcs::verification::ApprovalVerifier>());

    arcs::verification::VerificationContext context;
    context.policy = &policy;
    context.schema_registry = &registry;
    context.permissions.capabilities = {"exec:report_emit"};
    context.permissions.scopes = {"task_id:t_happy"};

    const auto report = engine.run_all(option, context);
    require(report.status == arcs::verification::CheckStatus::Pass, "happy path verification should pass");
    require(approval.payload.at("decision") == "approve", "happy path approval should be approve");
}

void run_smoke_tests()
{
    std::size_t passed = 0;

    run_named_smoke_test("schema registry smoke test", run_schema_registry_smoke_test, passed);
    run_named_smoke_test("schema registry size test", run_schema_registry_size_smoke_test, passed);
    run_named_smoke_test("store smoke test", run_store_smoke_test, passed);
    run_named_smoke_test("store duplicate version reject", run_store_duplicate_version_reject_smoke_test, passed);
    run_named_smoke_test("store wrong head reject", run_store_wrong_head_reject_smoke_test, passed);
    run_named_smoke_test("approval smoke test", run_approval_smoke_test, passed);
    run_named_smoke_test("materializer smoke test", run_materializer_smoke_test, passed);
    run_named_smoke_test("task reducer draft test", run_task_reducer_draft_smoke_test, passed);
    run_named_smoke_test("task reducer approved test", run_task_reducer_approved_smoke_test, passed);
    run_named_smoke_test("task reducer projection test", run_reducer_smoke_test, passed);
    run_named_smoke_test("approval reducer default test", run_approval_reducer_default_smoke_test, passed);
    run_named_smoke_test("approval reducer revoke test", run_approval_reducer_revoke_smoke_test, passed);
    run_named_smoke_test("verification pass test", run_verification_smoke_test, passed);
    run_named_smoke_test("verification unknown test", run_verification_unknown_smoke_test, passed);
    run_named_smoke_test("happy path smoke test", run_happy_path_smoke_test, passed);

    require(passed == 15, "expected 15 smoke tests to run");
    std::cout << "Smoke tests: " << passed << "/15 OK\n\n";
}

} // namespace

int main()
{
    try
    {
        run_smoke_tests();
    }
    catch (const std::exception& e)
    {
        std::cerr << "Smoke tests failed: " << e.what() << '\n';
        return 1;
    }

    const std::filesystem::path schema_path = std::filesystem::path(ARCS_SOURCE_DIR) / "schemas/v1/task.schema.json";

    nlohmann::json schema_json;
    if (!load_json_file(schema_path, schema_json))
    {
        std::cerr << "Failed to load schema: " << schema_path << '\n';
        return 1;
    }

    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = schema_json.value("$id", "arcs.task.v1");
    entry.document = schema_json;
    entry.source_path = schema_path;

    if (!registry.register_schema(entry))
    {
        std::cerr << "Failed to register schema: " << entry.id << '\n';
        return 1;
    }

    const nlohmann::json valid_task = {
        {"title", "Phase 2 Demo"},
        {"description", "Task validation should pass"},
        {"priority", "high"}
    };

    const nlohmann::json invalid_task = {
        {"description", "Missing title and invalid priority"},
        {"priority", "urgent"}
    };

    std::cout << "Schema loaded from: " << schema_path << '\n';
    std::cout << "Schema id: " << entry.id << "\n\n";

    std::cout << "Valid task:\n";
    print_result(arcs::schema::Validator::validate(valid_task, entry.id, registry));

    std::cout << "\nInvalid task:\n";
    print_result(arcs::schema::Validator::validate(invalid_task, entry.id, registry));

    return 0;
}
