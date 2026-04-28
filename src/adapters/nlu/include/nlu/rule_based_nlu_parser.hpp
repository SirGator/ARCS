// src/nlu/rule_based_nlu_parser.hpp

#pragma once
#include "nlu_parser.hpp"

namespace arcs::nlu {

class RuleBasedNluParser : public INluParser {
public:
    NluResult parse(const std::string& text) override;
};

}