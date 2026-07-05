/**
 * @file time_source.hpp
 * @brief Abstraction over "current time" so reducers can be tested with a
 * controllable clock.
 */
#pragma once
#include <string>

namespace arcs::reducer {

/** @brief ISO-8601 UTC timestamp string used throughout the reducer module. */
using Timestamp = std::string;

/**
 * @brief Interface for obtaining the current time, allowing production code
 * to use a real clock and tests to inject a fixed/mock one.
 */
class ITimeSource {
public:
    virtual ~ITimeSource() = default;
    /** @brief Returns the current timestamp. */
    virtual Timestamp now() const = 0;
};

} // namespace arcs::reducer
