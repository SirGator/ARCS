#include "store/store_memory.hpp"

#include "store/head_tracker.hpp"
#include "store/optimistic_lock.hpp"

#include <algorithm>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

namespace {

using arcs::artifact::ArtifactVersion;
using arcs::event::Event;

void ensure_version_insertable_impl(
    const ArtifactVersion& version,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id)
{
    if (version.artifact_id.empty()) {
        throw arcs::store::CommitRejectedError("artifact version rejected: artifact_id is empty");
    }

    if (version.version_id.empty()) {
        throw arcs::store::CommitRejectedError("artifact version rejected: version_id is empty");
    }

    if (versions_by_version_id.find(version.version_id) != versions_by_version_id.end()) {
        throw arcs::store::CommitRejectedError(
            "artifact version rejected: duplicate version_id '" + version.version_id + "'");
    }
}

void append_artifact_to_state_impl(
    const ArtifactVersion& version,
    std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::vector<std::string>>& version_ids_by_artifact_id)
{
    versions_by_version_id.emplace(version.version_id, version);
    version_ids_by_artifact_id[version.artifact_id].push_back(version.version_id);
}

void ensure_event_insertable_impl(
    const Event& event,
    const std::unordered_set<std::string>& known_event_ids)
{
    if (event.event_id.empty()) {
        throw arcs::store::CommitRejectedError("event rejected: event_id is empty");
    }

    if (event.event_type.empty()) {
        throw arcs::store::CommitRejectedError(
            "event rejected: event_type is empty for event_id '" + event.event_id + "'");
    }

    if (known_event_ids.find(event.event_id) != known_event_ids.end()) {
        throw arcs::store::CommitRejectedError(
            "event rejected: duplicate event_id '" + event.event_id + "'");
    }
}

void ensure_bundle_locally_consistent_impl(const arcs::store::CommitBundle& bundle)
{
    if (bundle.versions.empty()) {
        throw arcs::store::CommitRejectedError("commit rejected: no versions");
    }

    if (bundle.events.empty()) {
        throw arcs::store::CommitRejectedError("commit rejected: no events");
    }

    std::unordered_set<std::string> bundle_version_ids;
    for (const auto& pending : bundle.versions) {
        const auto& version = pending.version;

        if (!bundle_version_ids.insert(version.version_id).second) {
            throw arcs::store::CommitRejectedError(
                "commit rejected: duplicate version_id inside bundle '" +
                version.version_id + "'");
        }
    }

    std::unordered_set<std::string> bundle_event_ids;
    for (const auto& event : bundle.events) {
        if (!bundle_event_ids.insert(event.event_id).second) {
            throw arcs::store::CommitRejectedError(
                "commit rejected: duplicate event_id inside bundle '" +
                event.event_id + "'");
        }
    }

}

} // namespace

namespace arcs::store {

void StoreMemory::ensure_version_insertable(
    const ArtifactVersion& version,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id)
{
    ensure_version_insertable_impl(version, versions_by_version_id);
}

void StoreMemory::ensure_version_insertable(const ArtifactVersion& version) const
{
    ensure_version_insertable_impl(version, versions_by_version_id_);
}

void StoreMemory::append_artifact_to_state(
    const ArtifactVersion& version,
    std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::vector<std::string>>& version_ids_by_artifact_id)
{
    append_artifact_to_state_impl(version, versions_by_version_id, version_ids_by_artifact_id);
}

void StoreMemory::ensure_event_insertable(
    const Event& event,
    const std::unordered_set<std::string>& known_event_ids)
{
    ensure_event_insertable_impl(event, known_event_ids);
}

void StoreMemory::ensure_bundle_locally_consistent(const CommitBundle& bundle) const
{
    ensure_bundle_locally_consistent_impl(bundle);
}

StoreMemory::StoreMemory() = default;

StoreMemory::~StoreMemory() = default;

void StoreMemory::append_artifact(const ArtifactVersion& version)
{
    ensure_version_insertable(version, versions_by_version_id_);
    append_artifact_to_state(
        version,
        versions_by_version_id_,
        version_ids_by_artifact_id_);
}

void StoreMemory::append_event(const Event& event)
{
    ensure_event_insertable(event, event_ids_);
    event_log_.push_back(event);
    event_ids_.insert(event.event_id);
}

void StoreMemory::commit(const CommitBundle& bundle)
{
    ensure_bundle_locally_consistent(bundle);
    optimistic_lock::validate_bundle(bundle, *this);

    auto versions_copy = versions_by_version_id_;
    auto artifact_versions_copy = version_ids_by_artifact_id_;
    auto events_copy = event_log_;
    auto event_ids_copy = event_ids_;
    auto heads_copy = head_by_artifact_id_;

    for (const auto& pending : bundle.versions) {
        ensure_version_insertable(pending.version, versions_copy);
        append_artifact_to_state(
            pending.version,
            versions_copy,
            artifact_versions_copy);
    }

    for (const auto& event : bundle.events) {
        ensure_event_insertable(event, event_ids_copy);
        events_copy.push_back(event);
        event_ids_copy.insert(event.event_id);
    }

    head_tracker::apply_events(bundle.events, versions_copy, heads_copy);

    versions_by_version_id_ = std::move(versions_copy);
    version_ids_by_artifact_id_ = std::move(artifact_versions_copy);
    event_log_ = std::move(events_copy);
    event_ids_ = std::move(event_ids_copy);
    head_by_artifact_id_ = std::move(heads_copy);
}

ArtifactVersion StoreMemory::get(const std::string& artifact_id) const
{
    auto head_it = head_by_artifact_id_.find(artifact_id);
    if (head_it == head_by_artifact_id_.end()) {
        throw NotFoundError(
            "store get failed: no head found for artifact_id '" + artifact_id + "'");
    }

    auto version_it = versions_by_version_id_.find(head_it->second);
    if (version_it == versions_by_version_id_.end()) {
        throw StoreError(
            "store corrupted: head version '" + head_it->second +
            "' for artifact_id '" + artifact_id + "' does not exist");
    }

    return version_it->second;
}

ArtifactVersion StoreMemory::get_version(const std::string& version_id) const
{
    auto it = versions_by_version_id_.find(version_id);
    if (it == versions_by_version_id_.end()) {
        throw NotFoundError(
            "store get_version failed: unknown version_id '" + version_id + "'");
    }

    return it->second;
}

std::vector<ArtifactVersion> StoreMemory::list(const ListQuery& query) const
{
    std::vector<ArtifactVersion> result;
    result.reserve(versions_by_version_id_.size());

    for (const auto& [version_id, version] : versions_by_version_id_) {
        (void)version_id;

        if (query.type.has_value() && version.type != *query.type) {
            continue;
        }

        if (query.stream_key.has_value() && version.stream_key != *query.stream_key) {
            continue;
        }

        result.push_back(version);
    }

    std::sort(result.begin(), result.end(),
        [](const ArtifactVersion& a, const ArtifactVersion& b) {
            if (a.artifact_id != b.artifact_id) {
                return a.artifact_id < b.artifact_id;
            }
            return a.version_id < b.version_id;
        });

    return result;
}

std::vector<Event> StoreMemory::list_events(
    const std::optional<std::string>& stream_key) const
{
    if (!stream_key.has_value()) {
        return event_log_; 
    }

    std::vector<Event> result;
    result.reserve(event_log_.size());

    for (const auto& event : event_log_) {
        if (event.stream_key == *stream_key) {
            result.push_back(event);
        }
    }

    return result;
}

bool StoreMemory::has_artifact(const std::string& artifact_id) const
{
    return version_ids_by_artifact_id_.find(artifact_id) != version_ids_by_artifact_id_.end();
}

bool StoreMemory::has_version(const std::string& version_id) const
{
    return versions_by_version_id_.find(version_id) != versions_by_version_id_.end();
}

std::optional<std::string> StoreMemory::current_head_version_id(
    const std::string& artifact_id) const
{
    auto it = head_by_artifact_id_.find(artifact_id);
    if (it == head_by_artifact_id_.end()) {
        return std::nullopt;
    }

    return it->second;
}

} // namespace arcs::store
