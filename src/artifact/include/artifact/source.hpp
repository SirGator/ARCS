#pragma once
#include <string>

namespace arcs {

struct SourceRef {
    std::string kind; // chat | file | api | sensor | timer | internal
    std::string ref;
};

}