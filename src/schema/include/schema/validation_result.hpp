#pragma once

#include <string>
#include <vector>

namespace arcs::schema {

struct ValidationError {
    std::string path;
    std::string message;
};

struct ValidationResult {
    bool valid = false;
    std::string schema_id;
    std::vector<ValidationError> errors;
};

} // namespace arcs::schema
