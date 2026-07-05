/**
 * @file event.hpp
 * @brief Defines the Event data model used to record what happened in the
 *        system as an append-only, hash-chained stream of entries.
 */

#pragma once

#include <string>
#include <vector>
#include <nlohmann/json.hpp>

#include "artifact/actor.hpp"

namespace arcs::event {

/**
 * @brief Reference from an event to a specific artifact version, tagged
 *        with the role that artifact plays in the event (e.g. "subject",
 *        "cause").
 */
struct EventRef {
    std::string artifact_id;
    std::string version_id;
    std::string role;
};

/**
 * @brief A single entry in the ARCS event log.
 *
 * Events are chained via prev_hash to form a tamper-evident, per-stream
 * history, and carry references to the artifacts they relate to along
 * with an arbitrary JSON payload.
 */
struct Event {
    std::string event_id;
    std::string event_type;
    std::string ts;

    arcs::artifact::ActorRef actor;
    std::vector<EventRef> refs;

    std::string stream_key;
    nlohmann::json payload;

    std::string prev_hash;
};

} // namespace arcs::event
