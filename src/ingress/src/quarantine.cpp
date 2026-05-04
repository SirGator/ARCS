#include "ingress/quarantine.hpp"

#include <utility>

namespace arcs::ingress {

void QuarantineStore::store(QuarantinedEvent event)
{
    events_.push_back(std::move(event));
}

const std::vector<QuarantinedEvent>& QuarantineStore::events() const
{
    return events_;
}

std::size_t QuarantineStore::count() const
{
    return events_.size();
}

void QuarantineStore::clear()
{
    events_.clear();
}

} // namespace arcs::ingress
