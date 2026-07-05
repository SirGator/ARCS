/**
 * @file commit_service.cpp
 * @brief Implements CommitService, which builds commit bundles (artifact
 * versions plus their semantic events) and commits them to a store.
 */
#include "core/commit/commit_service.hpp"

namespace arcs::core::commit {
namespace {

/**
 * @brief Appends the versions and events of one commit bundle onto another.
 * @param destination Bundle to append into.
 * @param source Bundle whose contents are appended.
 */
void append_bundle(
    arcs::store::commit::CommitBundle& destination,
    const arcs::store::commit::CommitBundle& source)
{
    destination.versions.insert(destination.versions.end(), source.versions.begin(), source.versions.end());
    destination.events.insert(destination.events.end(), source.events.begin(), source.events.end());
}

} // namespace

CommitContext CommitService::make_context(const std::string& stage) const
{
    return CommitContext{
        .commit_id = "c_" + arcs::artifact::ids::new_event_id(),
        .stage = stage,
    };
}

/**
 * @brief Builds a commit bundle for a set of artifacts: each artifact
 * becomes a pending version, plus a semantic event of the given type and a
 * "head_advanced" event.
 * @param event_type Semantic event type to emit for each artifact.
 * @param artifacts Artifacts to include in the bundle.
 * @param timestamp Timestamp to stamp on the generated events.
 * @return The resulting CommitBundle.
 */
arcs::store::commit::CommitBundle CommitService::make_bundle(
    const std::string& event_type,
    const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
    const std::string& timestamp) const
{
    return make_bundle(make_context(event_type), event_type, artifacts, timestamp);
}

arcs::store::commit::CommitBundle CommitService::make_bundle(
    const CommitContext& commit_context,
    const std::string& event_type,
    const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
    const std::string& timestamp) const
{
    arcs::store::commit::CommitBundle bundle{};
    for (const auto& artifact : artifacts) {
        bundle.versions.push_back(arcs::store::commit::PendingVersion{artifact, std::nullopt});
        bundle.events.push_back(semantic_event_factory_.make_event(event_type, artifact, timestamp, commit_context));
        bundle.events.push_back(semantic_event_factory_.make_event("head_advanced", artifact, timestamp, commit_context));
    }
    return bundle;
}

/**
 * @brief Builds a commit bundle from the given artifacts and, if
 * non-empty, commits it to the store and appends it to a running
 * persisted bundle. No-op if there are no artifacts to commit.
 * @param store Store to commit the bundle to.
 * @param persisted_bundle Accumulator that the committed bundle is
 *        appended to.
 * @param event_type Semantic event type to emit for each artifact.
 * @param artifacts Artifacts to commit.
 * @param timestamp Timestamp to stamp on the generated events.
 */
void CommitService::commit_and_collect(
    arcs::store::IStore& store,
    arcs::store::commit::CommitBundle& persisted_bundle,
    const std::string& event_type,
    const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
    const std::string& timestamp) const
{
    commit_and_collect(store, persisted_bundle, make_context(event_type), event_type, artifacts, timestamp);
}

void CommitService::commit_and_collect(
    arcs::store::IStore& store,
    arcs::store::commit::CommitBundle& persisted_bundle,
    const CommitContext& commit_context,
    const std::string& event_type,
    const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
    const std::string& timestamp) const
{
    auto bundle = make_bundle(commit_context, event_type, artifacts, timestamp);
    if (bundle.versions.empty()) {
        return;
    }
    store.commit(bundle);
    append_bundle(persisted_bundle, bundle);
}

} // namespace arcs::core::commit
