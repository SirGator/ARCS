/**
 * @file json.hpp
 * @brief Declares nlohmann::json (de)serialization functions for the
 *        Event and EventRef types.
 */

#pragma once

#include <nlohmann/json.hpp>

#include "event/event.hpp"

namespace arcs::event {

using nlohmann::json;

/**
 * @brief Serializes an EventRef to JSON.
 * @param j Output JSON value to populate.
 * @param v The EventRef to serialize.
 */
void to_json(json& j, const EventRef& v);

/**
 * @brief Deserializes an EventRef from JSON.
 * @param j Input JSON value to read from.
 * @param v The EventRef to populate.
 */
void from_json(const json& j, EventRef& v);

/**
 * @brief Serializes an Event to JSON.
 * @param j Output JSON value to populate.
 * @param v The Event to serialize.
 */
void to_json(json& j, const Event& v);

/**
 * @brief Deserializes an Event from JSON, defaulting payload to an empty
 *        object and prev_hash to empty if absent.
 * @param j Input JSON value to read from.
 * @param v The Event to populate.
 */
void from_json(const json& j, Event& v);

} // namespace arcs::event
