#pragma once
    
#include<string>
#include<filesystem>

#include <nlohmann/json.hpp>
    
namespace arcs::schema{
    struct SchemaEntry{

        // die schema id 
        std::string id;
        // das json document an sich  
        nlohmann::json document;
        // wo ist das jsom dukoment damit man später weiß wo man schauen soll
        std::filesystem::path source_path;
    };

}