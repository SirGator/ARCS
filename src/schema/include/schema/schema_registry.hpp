#pragma once

#include <string>
#include <filesystem>
#include <unordered_map>

#include <nlohmann/json.hpp>

#include "schema_types.hpp"

namespace arcs::schema{

    class SchemaRegistry{
    public:
        
        // ob register_schema waht oder falsch ist mit denn parametern const SchemaEntry & entry
        bool register_schema(const SchemaEntry& entry);

        // Prüft gibt es ein Schema mit dieser ID
        // const am Ende damit die Registry nicht verändert wird
        bool has_schema(const std::string& id) const;

        // Holt ein Schema mit eine, pointer 
        // Kopieren von JSON
        // schnell
        // lifetime bleibt Registry-owned
        // Registry soll immutable Lookup-Table sein.
        const SchemaEntry* find_schema(const std::string& id) const;

        // Gibt Anzahl registrierter Schemas zurück.
        size_t size() const;

    private:
        
        // Container der alle Schemas hält
        std::unordered_map<std::string, SchemaEntry> schemas_;

    };
    


};