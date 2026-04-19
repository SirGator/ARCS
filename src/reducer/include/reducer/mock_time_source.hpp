#pragma once

#include <string>

#include "reducer/time_source.hpp"

namespace arcs::reducer {

class MockTimeSource : public ITimeSource {
public:
    explicit MockTimeSource(Timestamp fixed_now);

    Timestamp now() const override;

    void set_now(Timestamp value);

private:
    Timestamp fixed_now_;
};

} // namespace arcs::reducer
