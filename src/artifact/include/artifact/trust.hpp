/**
 * @file trust.hpp
 * @brief Defines TrustInfo, capturing how much an artifact's content should
 *        be trusted and the class of entity that produced it.
 */
#pragma once
#include <string>

namespace arcs::artifact {

/**
 * @brief Trust classification attached to an artifact, combining a trust
 *        level (low/medium/high) with the class of source that produced it.
 */
struct TrustInfo {
    std::string level;        // low | medium | high
    std::string source_class; // human | system | model | external
};

}
