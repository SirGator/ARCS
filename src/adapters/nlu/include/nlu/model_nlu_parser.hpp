// src/nlu/model_nlu_parser.hpp

#pragma once
#include "nlu_parser.hpp"

namespace arcs::nlu {

class ModelNluParser : public INluParser {
public:
    explicit ModelNluParser(const std::string& model_path);

    NluResult parse(const std::string& text) override;

private:
    std::string model_path_;
};

}