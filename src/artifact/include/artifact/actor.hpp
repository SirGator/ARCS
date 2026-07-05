/**
 * @file actor.hpp
 * @brief Defines ActorRef, the lightweight reference used across the
 *        artifact model to identify who or what produced an artifact
 *        (human, system, model, or executor).
 */
#pragma once
#include <string>

namespace arcs::artifact {

/**
 * @brief Identifies the actor responsible for creating or acting on an
 *        artifact, along with its type classification.
 */
struct ActorRef {
    std::string actor_type; // human | system | model | executor
    std::string id;
};

}
