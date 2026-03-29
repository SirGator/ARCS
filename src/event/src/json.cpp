#include "event/json.hpp"

#include "artifact/json.hpp"

namespace arcs::event {

void to_json(json& j, const EventRef& v) {
    j = json{
        {"artifact_id", v.artifact_id},
        {"version_id", v.version_id},
        {"role", v.role}
    };
}

void from_json(const json& j, EventRef& v) {
    j.at("artifact_id").get_to(v.artifact_id);
    j.at("version_id").get_to(v.version_id);
    j.at("role").get_to(v.role);
}

void to_json(json& j, const Event& v) {
    j = json{
        {"event_id", v.event_id},
        {"event_type", v.event_type},
        {"ts", v.ts},
        {"actor", v.actor},
        {"refs", v.refs},
        {"stream_key", v.stream_key},
        {"payload", v.payload},
        {"prev_hash", v.prev_hash}
    };
}

void from_json(const json& j, Event& v) {
    j.at("event_id").get_to(v.event_id);
    j.at("event_type").get_to(v.event_type);
    j.at("ts").get_to(v.ts);
    j.at("actor").get_to(v.actor);
    j.at("refs").get_to(v.refs);
    j.at("stream_key").get_to(v.stream_key);

    if (j.contains("payload")) {
        v.payload = j.at("payload");
    } else {
        v.payload = json::object();
    }

    if (j.contains("prev_hash")) {
        j.at("prev_hash").get_to(v.prev_hash);
    } else {
        v.prev_hash.clear();
    }
}

} // namespace arcs::event
