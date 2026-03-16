#include <iostream>

#include <nlohmann/json.hpp>

#include "schema/schema_loader.hpp"
#include "schema/schema_registry.hpp"
#include "schema/validator.hpp"

int main()
{
    using namespace arcs::schema;

    SchemaRegistry registry;

    // 1. Schema laden
    auto schema_opt = SchemaLoader::load_from_file(
        "schemas/v1/task.schema.json"
    );

    if (!schema_opt)
    {
        std::cout << "Schema load failed\n";
        return 1;
    }

    // 2. registrieren
    if (!registry.register_schema(*schema_opt))
    {
        std::cout << "Schema register failed\n";
        return 1;
    }

    std::cout << "Schema registered. Count = "
              << registry.size() << "\n";

    // 3. Artifact bauen
    nlohmann::json artifact = {
        {"id", "task_1"},
        {"kind", "task"}
    };

    // 4. validieren
    auto result = Validator::validate(
        artifact,
        schema_opt->id,
        registry
    );

    // 5. Ergebnis anzeigen
    if (result.valid)
    {
        std::cout << "VALID\n";
    }
    else
    {
        std::cout << "INVALID\n";

        for (const auto& err : result.errors)
        {
            std::cout << "Error: " << err << "\n";
        }
    }

    return 0;
}