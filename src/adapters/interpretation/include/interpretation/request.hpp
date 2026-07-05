/**
 * @file request.hpp
 * @brief Defines the request payload sent to the external interpretation
 *        worker.
 */

#pragma once

#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace arcs::interpretation {

/**
 * @brief Complete request to the external interpretation worker.
 *
 * Everything the parser needs is carried in a single request object, so
 * the worker never needs to perform an extra lookup.
 */
struct InterpretationRequest {
    /// Caller-assigned correlation ID for logs and follow-up questions.
    std::string request_id;
    /// The unparsed raw text that should be interpreted.
    std::string raw_input;
    /// Target schema that the response is expected to conform to.
    std::string schema_id;
    /// The schema itself, sent inline so the parser can work without an extra lookup.
    nlohmann::json schema;
    /// Runtime context for the interpretation, e.g. language, timezone, and current time.
    nlohmann::json context;
    /// Controls prompt/parser behavior, e.g. mode or temperature.
    nlohmann::json prompt_config;
};

} // namespace arcs::interpretation
