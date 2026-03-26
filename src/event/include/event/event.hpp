#pragma once

#include <string>
#include <vector>
#include <nlohmann/json.hpp>

#include "artifact/actor.hpp"

namespace arcs {

struct EventRef {
    std::string artifact_id;
    std::string version_id;
    std::string role;
};

struct Event {
    std::string event_id;
    std::string event_type;
    std::string ts;

    ActorRef actor;
    std::vector<EventRef> refs;

    std::string stream_key;
    nlohmann::json payload;

    std::string prev_hash;
};

} // namespace arcs