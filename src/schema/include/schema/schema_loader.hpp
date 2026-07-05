/**
 * @file schema_loader.hpp
 * @brief Declares SchemaLoader, responsible for reading a JSON schema
 *        document off disk into a SchemaEntry.
 */

#pragma once

#include <filesystem>
#include <optional>

#include "schema/schema_types.hpp"

namespace arcs::schema {

/**
 * @brief Loads JSON schema documents from the filesystem.
 */
class SchemaLoader {
public:
    /**
     * @brief Reads and parses a JSON schema file, extracting its `$id`.
     * @param file_path Path to the JSON schema file to load.
     * @return The loaded schema entry, or std::nullopt if the file could
     *         not be opened, was not valid JSON, or lacked a string `$id`.
     */
    static std::optional<SchemaEntry> load_from_file(const std::filesystem::path& file_path);
};

} // namespace arcs::schema
