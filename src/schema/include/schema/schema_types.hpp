/**
 * @file schema_types.hpp
 * @brief Defines the SchemaEntry type used to represent a loaded JSON
 *        schema document throughout the schema module.
 */

#pragma once

#include <filesystem>
#include <string>

#include <nlohmann/json.hpp>

namespace arcs::schema {

/**
 * @brief A single loaded JSON schema, identified by its `$id` and paired
 *        with the file it was loaded from.
 */
struct SchemaEntry {
    /// The schema ID (taken from the document's `$id` field).
    std::string id;
    /// The JSON document itself.
    nlohmann::json document;
    /// Where the JSON document came from, so we know where to look later.
    std::filesystem::path source_path;
};

} // namespace arcs::schema
