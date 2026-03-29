#pragma once
#include <string>

namespace arcs::artifact {

struct ActorRef {
    std::string actor_type; // human | system | model | executor
    std::string id;
};

}
