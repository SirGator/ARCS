/**
 * @file mock_time_source.hpp
 * @brief Test double for ITimeSource that returns a fixed, adjustable time.
 */
#pragma once

#include <string>

#include "reducer/time_source.hpp"

namespace arcs::reducer {

/**
 * @brief ITimeSource implementation that always returns a fixed timestamp,
 * allowing tests to control "now" deterministically.
 */
class MockTimeSource : public ITimeSource {
public:
    /**
     * @brief Constructs a mock time source with an initial fixed time.
     * @param fixed_now The timestamp to return from now().
     */
    explicit MockTimeSource(Timestamp fixed_now);

    /** @brief Returns the currently configured fixed timestamp. */
    Timestamp now() const override;

    /**
     * @brief Updates the fixed timestamp returned by now().
     * @param value The new timestamp to use.
     */
    void set_now(Timestamp value);

private:
    Timestamp fixed_now_;
};

} // namespace arcs::reducer
