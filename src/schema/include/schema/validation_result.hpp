/**
 * @file validation_result.hpp
 * @brief Defines the types used to report the outcome of validating a JSON
 *        document against a schema.
 */

#pragma once

#include <string>
#include <vector>

namespace arcs::schema {

/**
 * @brief A single validation failure, located by JSON pointer path.
 */
struct ValidationError {
    std::string path;
    std::string message;
};

/**
 * @brief Outcome of validating a document against a specific schema,
 *        including the list of validation errors, if any.
 */
struct ValidationResult {
    bool valid = false;
    std::string schema_id;
    std::vector<ValidationError> errors;
};

} // namespace arcs::schema
