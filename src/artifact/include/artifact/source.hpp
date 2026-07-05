/**
 * @file source.hpp
 * @brief Defines SourceRef, describing where an artifact's data originated
 *        from (e.g. chat, file, api, sensor, timer, internal).
 */
#pragma once
#include <string>

namespace arcs::artifact {

/**
 * @brief Reference to the origin of an artifact's content, identifying both
 *        the kind of source and a concrete pointer to it (e.g. a path or URL).
 */
struct SourceRef {
    std::string kind; // chat | file | api | sensor | timer | internal
    std::string ref;
};

}
