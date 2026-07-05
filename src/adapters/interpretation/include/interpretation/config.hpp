/**
 * @file config.hpp
 * @brief Configuration for connecting to the external interpretation worker.
 */

#pragma once

#include <optional>
#include <string>

namespace arcs::interpretation {

/**
 * @brief Minimal configuration for the external interpretation worker.
 *
 * ARCS only needs a single endpoint to talk to the worker.
 */
struct InterpretationApiConfig {
    /// Full URL to POST /interpret.
    std::optional<std::string> interpret_api_url;
};

} // namespace arcs::interpretation
