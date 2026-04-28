// src/nlu/nlu_parser.hpp

#pragma once
#include "nlu_result.hpp"
#include <string>

namespace arcs::nlu {

class INluParser {
public:
    virtual ~INluParser() = default;

    virtual NluResult parse(const std::string& text) = 0;
};

}