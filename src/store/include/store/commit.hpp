#pragma once

#include "artifact/artifact.hpp"
#include "event/event.hpp"

#include <optional>
#include <string>
#include <vector>

namespace arcs::store::commit {

using arcs::artifact::ArtifactVersion;
using arcs::event::Event;

// Eine geplante neue Version plus optionaler Optimistic-Lock-Kontext.
// expected_head_version_id bedeutet:
// "Ich darf diese neue Version nur committen, wenn der aktuelle Head
//  dieses Artefakts genau diese Version ist."
struct PendingVersion {
    ArtifactVersion version;
    std::optional<std::string> expected_head_version_id;
};

// Das atomare Schreib-Bundle für den Store.
// Entweder alles wird übernommen, oder nichts.
struct CommitBundle {
    std::vector<PendingVersion> versions;
    std::vector<Event> events;
};

} // namespace arcs::store::commit
