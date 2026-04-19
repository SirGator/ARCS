#include <filesystem>
#include <fstream>
#include <iostream>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "event/event.hpp"
#include "reducer/approval_state_reducer.hpp"
#include "reducer/task_state_reducer.hpp"
#include "schema/schema_registry.hpp"
#include "schema/validator.hpp"
#include "store/store_memory.hpp"

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

void run_smoke_tests()
{
    run_schema_registry_smoke_test();
    run_store_smoke_test();
    run_reducer_smoke_test();
}

} // namespace

int main()
{
    try
    {
        run_smoke_tests();
        std::cout << "Smoke tests: OK\n\n";
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
