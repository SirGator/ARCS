/**
 * @file quarantine.hpp
 * @brief Defines storage for ingress events that failed normalization,
 *        validation, or routing, so they can be inspected or replayed
 *        later instead of being silently dropped.
 */
#pragma once

#include <string>
#include <vector>

#include "artifact/artifact.hpp"

namespace arcs::ingress {

/**
 * @brief An ingress artifact that was rejected somewhere in the pipeline,
 *        along with why, when, and at which stage it was rejected.
 */
struct QuarantinedEvent {
    arcs::artifact::ArtifactVersion artifact;
    std::string rejection_reason;
    std::string rejected_at;
    std::string rejection_stage;  // "normalize" | "validate" | "route"
};

// In-Memory Quarantine-Speicher für fehlgeschlagene Ingress-Events.
/**
 * @brief In-memory store collecting QuarantinedEvent records for ingress
 *        events that failed processing.
 */
class QuarantineStore {
public:
    /**
     * @brief Adds a quarantined event to the store.
     * @param event The event to store (moved in).
     */
    void store(QuarantinedEvent event);

    /**
     * @brief Returns all events currently held in the store.
     * @return A reference to the stored quarantined events.
     */
    const std::vector<QuarantinedEvent>& events() const;

    /**
     * @brief Returns the number of events currently held in the store.
     * @return The count of quarantined events.
     */
    std::size_t count() const;

    /// @brief Removes all events from the store.
    void clear();

private:
    std::vector<QuarantinedEvent> events_;
};

} // namespace arcs::ingress
