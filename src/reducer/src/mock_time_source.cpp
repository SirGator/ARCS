#include "reducer/mock_time_source.hpp"

#include <utility>

namespace arcs::reducer {

MockTimeSource::MockTimeSource(Timestamp fixed_now)
    : fixed_now_(std::move(fixed_now))
{
}

Timestamp MockTimeSource::now() const
{
    return fixed_now_;
}

void MockTimeSource::set_now(Timestamp value)
{
    fixed_now_ = std::move(value);
}

} // namespace arcs::reducer
