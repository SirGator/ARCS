#pragma once

#include <filesystem>
#include <string>

#include <nlohmann/json.hpp>

namespace arcs::schema {

struct SchemaEntry {
    // The schema ID.
    std::string id;
    // The JSON document itself.
    nlohmann::json document;
    // Where the JSON document came from, so we know where to look later.
    std::filesystem::path source_path;
};

} // namespace arcs::schema
