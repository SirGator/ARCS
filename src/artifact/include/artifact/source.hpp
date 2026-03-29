#pragma once
#include <string>

namespace arcs::artifact {

struct SourceRef {
    std::string kind; // chat | file | api | sensor | timer | internal
    std::string ref;
};

}
