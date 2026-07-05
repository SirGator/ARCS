#pragma once

#include <string>

#include <nlohmann/json.hpp>

#include "adapters/common.hpp"

namespace arcs::core::intake {

struct AdapterSubmission {
    std::string adapter_id;
    arcs::adapters::AdapterKind adapter_kind{arcs::adapters::AdapterKind::Input};
    std::string schema_id;
    std::string artifact_type;
    std::string stream_key;

    nlohmann::json payload = nlohmann::json::object();
    nlohmann::json metadata = nlohmann::json::object();

    std::string actor_type;
    std::string actor_id;
    std::string source_kind;
    std::string source_ref;
};

} // namespace arcs::core::intake
