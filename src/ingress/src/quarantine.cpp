/**
 * @file quarantine.cpp
 * @brief Implements QuarantineStore, an in-memory list of ingress events
 *        that failed processing at some stage of the pipeline.
 */
#include "ingress/quarantine.hpp"

#include <utility>

namespace arcs::ingress {

/**
 * @brief Adds a quarantined event to the store.
 * @param event The event to store (moved in).
 */
void QuarantineStore::store(QuarantinedEvent event)
{
    events_.push_back(std::move(event));
}

/**
 * @brief Returns all events currently held in the store.
 * @return A reference to the stored quarantined events.
 */
const std::vector<QuarantinedEvent>& QuarantineStore::events() const
{
    return events_;
}

/**
 * @brief Returns the number of events currently held in the store.
 * @return The count of quarantined events.
 */
std::size_t QuarantineStore::count() const
{
    return events_.size();
}

/// @brief Removes all events from the store.
void QuarantineStore::clear()
{
    events_.clear();
}

} // namespace arcs::ingress
