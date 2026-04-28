// src/nlu/nlu_result.hpp

#pragma once
#include <string>
#include <map>

namespace arcs::nlu {

enum class NluStatus {
    Parsed,
    Unknown,
    Ambiguous
};

struct NluResult {
    NluStatus status = NluStatus::Unknown;

    std::string intent; 
    double confidence = 0.0;

    std::map<std::string, std::string> entities;

    std::string raw_text;
    std::string parser_name;
};

}