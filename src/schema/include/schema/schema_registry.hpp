#pragma once

#include <filesystem>
#include <unordered_map>
#include <string>

#include <nlohmann/json.hpp>

#include "schema/schema_types.hpp"

namespace arcs::schema {

class SchemaRegistry {
public:
    // Returns whether `register_schema` succeeded for the given `SchemaEntry`.
    bool register_schema(const SchemaEntry& entry);

    // Checks whether a schema with this ID exists.
    // `const` means the registry is not modified.
    bool has_schema(const std::string& id) const;

    // Returns a pointer to a schema.
    // Avoids copying JSON, is fast, and keeps ownership with the registry.
    // The registry is intended to behave like an immutable lookup table.
    const SchemaEntry* find_schema(const std::string& id) const;

    // Returns the number of registered schemas.
    size_t size() const;

private:
    // Container holding all schemas.
    std::unordered_map<std::string, SchemaEntry> schemas_;

};

} // namespace arcs::schema
