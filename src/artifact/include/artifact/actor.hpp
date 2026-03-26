#pragma once
#include <string>

namespace arcs {

struct ActorRef {
    std::string actor_type; // human | system | model | executor
    std::string id;
};

}