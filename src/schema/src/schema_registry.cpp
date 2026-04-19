#include "schema/schema_registry.hpp"

namespace arcs::schema {

bool SchemaRegistry::register_schema(const SchemaEntry& entry)
{
    // Return false when the schema ID is empty.
    if (entry.id.empty())
        return false;

    if (schemas_.find(entry.id) != schemas_.end())
        return false;

    schemas_.emplace(entry.id, entry);
    return true;
}

bool SchemaRegistry::has_schema(const std::string& schema_id) const
{
    return schemas_.find(schema_id) != schemas_.end();
}

const SchemaEntry* SchemaRegistry::find_schema(const std::string& schema_id) const
{
    auto it = schemas_.find(schema_id);

    if (it == schemas_.end())
        return nullptr;

    return &it->second;
}

std::size_t SchemaRegistry::size() const
{
    return schemas_.size();
}

}
