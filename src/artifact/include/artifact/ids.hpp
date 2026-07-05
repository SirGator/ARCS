/**
 * @file ids.hpp
 * @brief Declares functions for generating unique identifiers used to
 *        label artifacts, artifact versions, and events.
 */
#pragma once

#include <string>

namespace arcs::artifact::ids {

/**
 * @brief Generates a new unique artifact identifier.
 * @return A unique string ID prefixed for artifacts.
 */
std::string new_artifact_id();

/**
 * @brief Generates a new unique artifact version identifier.
 * @return A unique string ID prefixed for artifact versions.
 */
std::string new_version_id();

/**
 * @brief Generates a new unique event identifier.
 * @return A unique string ID prefixed for events.
 */
std::string new_event_id();

} // namespace arcs::artifact::ids
