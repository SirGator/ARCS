#include "schema/schema_loader.hpp"

#include <fstream>

namespace arcs::schema {

std::optional<SchemaEntry> SchemaLoader::load_from_file(const std::filesystem::path& file_path)
{
    std::ifstream file(file_path);
    if (!file.is_open())
        return std::nullopt;

    nlohmann::json json_document;
    try
    {
        file >> json_document;
    }
    catch (...)
    {
        return std::nullopt;
    }

    if (!json_document.contains("$id") || !json_document["$id"].is_string())
        return std::nullopt;

    SchemaEntry entry;
    entry.id = json_document["$id"].get<std::string>();
    entry.document = json_document;
    entry.source_path = file_path;

    return entry;
}

}