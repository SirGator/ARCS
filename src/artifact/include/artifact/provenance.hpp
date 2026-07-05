/**
 * @file provenance.hpp
 * @brief Defines the provenance data attached to artifacts: which parent
 *        artifacts, rules, transforms and model invocations contributed to
 *        producing the current artifact version.
 */
#pragma once
#include <string>
#include <vector>

namespace arcs::artifact {

/**
 * @brief Records details of a single model invocation that contributed to
 *        an artifact, including a hash of the prompt/inputs and the raw
 *        output, for auditability without storing full model I/O.
 */
struct ModelUsage {
    std::string name;
    std::string prompt_hash;
    std::vector<std::string> inputs;
    double temperature{0.0};
    std::string raw_output_hash;
};

/**
 * @brief Traces how an artifact was derived: its parent artifacts, the
 *        rules applied, any model usages involved, and the name of the
 *        transform that produced it.
 */
struct Provenance {
    std::vector<std::string> parents;
    std::vector<std::string> rules_applied;
    std::vector<ModelUsage> models_used;
    std::string transform;
};

}
