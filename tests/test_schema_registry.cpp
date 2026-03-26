#include <stdexcept>
#include <string>

#include <nlohmann/json.hpp>

#include "schema/schema_registry.hpp"

namespace {

void require(bool condition, const std::string& message)
{
    if (!condition)
    {
        throw std::runtime_error(message);
    }
}

void test_register_and_find_schema()
{
    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = "arcs.test.schema.v1";
    entry.document = {{"$id", entry.id}, {"type", "object"}};
    entry.source_path = "schemas/v1/test.schema.json";

    require(registry.register_schema(entry), "first schema registration should succeed");
    require(registry.size() == 1, "registry size should be 1");
    require(registry.has_schema(entry.id), "registry should report schema present");

    const arcs::schema::SchemaEntry* found = registry.find_schema(entry.id);
    require(found != nullptr, "schema should be findable");
    require(found->id == entry.id, "found schema id mismatch");
    require(found->source_path == entry.source_path, "found schema path mismatch");
}

void test_reject_duplicate_schema()
{
    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.id = "arcs.test.schema.v1";
    entry.document = {{"$id", entry.id}};

    require(registry.register_schema(entry), "first schema registration should succeed");
    require(!registry.register_schema(entry), "duplicate schema registration should fail");
    require(registry.size() == 1, "registry size should stay 1");
}

void test_reject_empty_id()
{
    arcs::schema::SchemaRegistry registry;
    arcs::schema::SchemaEntry entry;
    entry.document = { {"type", "object"} };

    require(!registry.register_schema(entry), "empty schema id should be rejected");
    require(registry.size() == 0, "registry should stay empty");
}

} // namespace

int main()
{
    test_register_and_find_schema();
    test_reject_duplicate_schema();
    test_reject_empty_id();
    return 0;
}
