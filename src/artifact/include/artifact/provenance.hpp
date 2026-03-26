#pragma once
#include <string>
#include <vector>

namespace arcs {

struct ModelUsage {
    std::string name;
    std::string prompt_hash;
    std::vector<std::string> inputs;
    double temperature{0.0};
    std::string raw_output_hash;
};

struct Provenance {
    std::vector<std::string> parents;
    std::vector<std::string> rules_applied;
    std::vector<ModelUsage> models_used;
    std::string transform;
};

}