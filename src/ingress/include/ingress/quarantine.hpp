#pragma once

#include <string>
#include <vector>

#include "artifact/artifact.hpp"

namespace arcs::ingress {

struct QuarantinedEvent {
    arcs::artifact::ArtifactVersion artifact;
    std::string rejection_reason;
    std::string rejected_at;
    std::string rejection_stage;  // "normalize" | "validate" | "route"
};

// In-Memory Quarantine-Speicher für fehlgeschlagene Ingress-Events.
class QuarantineStore {
public:
    void store(QuarantinedEvent event);

    const std::vector<QuarantinedEvent>& events() const;

    std::size_t count() const;

    void clear();

private:
    std::vector<QuarantinedEvent> events_;
};

} // namespace arcs::ingress
