#pragma once

#include <nlohmann/json.hpp>

#include "event/event.hpp"

namespace arcs::event {

using nlohmann::json;

void to_json(json& j, const EventRef& v);
void from_json(const json& j, EventRef& v);

void to_json(json& j, const Event& v);
void from_json(const json& j, Event& v);

} // namespace arcs::event
