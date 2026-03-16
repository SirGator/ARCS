#pragma once

#include <optional>
#include <filesystem>

#include "schema/schema_types.hpp"

namespace arcs::schema {

    class SchemaLoader
    {
    public:
        static std::optional<SchemaEntry> load_from_file(const std::filesystem::path& file_path);
    };

}