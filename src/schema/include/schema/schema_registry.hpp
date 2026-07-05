/**
 * @file schema_registry.hpp
 * @brief Declares SchemaRegistry, an in-memory lookup table of loaded
 *        schemas keyed by schema ID.
 */

#pragma once

#include <filesystem>
#include <unordered_map>
#include <string>

#include <nlohmann/json.hpp>

#include "schema/schema_types.hpp"

namespace arcs::schema {

/**
 * @brief Holds the set of known schemas, keyed by schema ID, and acts as
 *        an immutable lookup table once populated.
 */
class SchemaRegistry {
public:
    /**
     * @brief Registers a schema entry under its ID.
     * @param entry The schema entry to register.
     * @return True on success; false if the entry's ID is empty or already
     *         registered.
     */
    bool register_schema(const SchemaEntry& entry);

    /**
     * @brief Checks whether a schema with this ID exists.
     * @param id The schema ID to look up.
     * @return True if a schema with this ID is registered.
     */
    bool has_schema(const std::string& id) const;

    /**
     * @brief Looks up a schema by ID without copying it.
     *
     * Returns a pointer rather than a copy to avoid duplicating the JSON
     * document; ownership stays with the registry.
     *
     * @param id The schema ID to look up.
     * @return Pointer to the matching schema entry, or nullptr if not found.
     */
    const SchemaEntry* find_schema(const std::string& id) const;

    /**
     * @brief Returns the number of registered schemas.
     */
    size_t size() const;

private:
    // Container holding all schemas.
    std::unordered_map<std::string, SchemaEntry> schemas_;

};

} // namespace arcs::schema
