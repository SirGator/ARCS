#pragma once

#include <string>
#include <vector>

namespace arcs::schema{

    // error path + nassege
    struct validationError{
        std::string path;
        std::string massege;
    };

        // ok / nicht ok 
    struct ValidationResult{
        bool valid = false;
        std::string schema_id;
        std::vector<std::string> errors;
    };
}