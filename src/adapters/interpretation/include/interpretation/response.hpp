/**
 * @file response.hpp
 * @brief Defines the response payload returned by the external
 *        interpretation worker.
 */

#pragma once

#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace arcs::interpretation {

/**
 * @brief Uniform response from the interpretation parser.
 *
 * `payload` holds the structured proposal; `error` is only set when
 * something went wrong.
 */
struct InterpretationResponse {
    /// Whether the request was accepted both technically and semantically.
    bool ok{false};
    /// Echo of the request ID, if the parser includes it.
    std::string request_id;
    /// Echo of the schema used, so callers can correlate response with request.
    std::string schema_id;
    /// Structured interpretation as a JSON object.
    nlohmann::json payload;
    /// Error message when processing was not successful.
    std::optional<std::string> error;
};

} // namespace arcs::interpretation
