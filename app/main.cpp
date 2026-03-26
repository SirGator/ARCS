#include <iostream>
#include <fstream>
#include <filesystem>

#include <nlohmann/json.hpp>

#include "schema/schema_registry.hpp"
#include "schema/validator.hpp"

namespace {

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

} // namespace

int main()
{
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
