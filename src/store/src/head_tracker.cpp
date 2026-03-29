#include "store/head_tracker.hpp"

namespace arcs::store::head_tracker {

namespace {



const EventRef* find_target_ref(const Event& event)
{
    for (const auto& ref : event.refs) {
        if (ref.role == "target") {
            return &ref;
        }
    }
    return nullptr;
}

void validate_target_ref(const Event& event, const EventRef& ref)
{
    if (ref.artifact_id.empty()) {
        throw CommitRejectedError(
            "head_advanced event '" + event.event_id +
            "' has empty target artifact_id");
    }

    if (ref.version_id.empty()) {
        throw CommitRejectedError(
            "head_advanced event '" + event.event_id +
            "' has empty target version_id");
    }
}

} // namespace

void apply_head_advanced_event(
    const Event& event,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id)
{
    if (event.event_type != "head_advanced") {
        throw CommitRejectedError(
            "apply_head_advanced_event called with non-head event_type '" +
            event.event_type + "'");
    }

    const EventRef* target = find_target_ref(event);
    if (target == nullptr) {
        throw CommitRejectedError(
            "head_advanced event '" + event.event_id +
            "' is missing target ref");
    }

    validate_target_ref(event, *target);

    auto version_it = versions_by_version_id.find(target->version_id);
    if (version_it == versions_by_version_id.end()) {
        throw CommitRejectedError(
            "head_advanced event '" + event.event_id +
            "' references unknown version_id '" + target->version_id + "'");
    }

    const ArtifactVersion& version = version_it->second;

    if (version.artifact_id != target->artifact_id) {
        throw CommitRejectedError(
            "head_advanced event '" + event.event_id +
            "' references artifact_id '" + target->artifact_id +
            "', but version '" + target->version_id +
            "' belongs to artifact_id '" + version.artifact_id + "'");
    }

    head_by_artifact_id[target->artifact_id] = target->version_id;
}

void apply_events(
    const std::vector<Event>& events,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id)
{
    for (const auto& event : events) {
        if (event.event_type != "head_advanced") {
            continue;
        }

        apply_head_advanced_event(
            event,
            versions_by_version_id,
            head_by_artifact_id);
    }
}

} // namespace arcs::store::head_tracker
