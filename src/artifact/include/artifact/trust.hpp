#pragma once
#include <string>

namespace arcs::artifact {

struct TrustInfo {
    std::string level;        // low | medium | high
    std::string source_class; // human | system | model | external
};

}
