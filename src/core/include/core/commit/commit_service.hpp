/**
 * @file commit_service.hpp
 * @brief Builds and persists commit bundles that group artifact versions with
 *        their semantic event into the store.
 */

#pragma once

#include <string>
#include <vector>

#include "artifact/ids.hpp"
#include "artifact/artifact.hpp"
#include "core/commit/semantic_event_factory.hpp"
#include "store/commit.hpp"
#include "store/store.hpp"

namespace arcs::core::commit {

/**
 * @brief Assembles commit bundles (artifact versions plus a semantic event)
 *        and optionally persists them to a store, tracking the persisted
 *        result for the caller.
 */
class CommitService {
public:
    CommitContext make_context(const std::string& stage) const;

    /**
     * @brief Builds a commit bundle containing the given artifact versions
     *        and a semantic event describing them, without persisting it.
     * @param event_type Type identifier for the semantic event to generate.
     * @param artifacts Artifact versions to include in the bundle.
     * @param timestamp Timestamp to stamp on the generated event.
     * @return The assembled, not-yet-persisted commit bundle.
     */
    arcs::store::commit::CommitBundle make_bundle(
        const std::string& event_type,
        const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
        const std::string& timestamp) const;

    arcs::store::commit::CommitBundle make_bundle(
        const CommitContext& commit_context,
        const std::string& event_type,
        const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
        const std::string& timestamp) const;

    /**
     * @brief Builds a commit bundle and commits it to the given store,
     *        appending the persisted result into @p persisted_bundle.
     * @param store Store to commit the bundle into.
     * @param persisted_bundle Accumulator that the persisted bundle contents
     *        are appended to.
     * @param event_type Type identifier for the semantic event to generate.
     * @param artifacts Artifact versions to include in the bundle.
     * @param timestamp Timestamp to stamp on the generated event.
     */
    void commit_and_collect(
        arcs::store::IStore& store,
        arcs::store::commit::CommitBundle& persisted_bundle,
        const std::string& event_type,
        const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
        const std::string& timestamp) const;

    void commit_and_collect(
        arcs::store::IStore& store,
        arcs::store::commit::CommitBundle& persisted_bundle,
        const CommitContext& commit_context,
        const std::string& event_type,
        const std::vector<arcs::artifact::ArtifactVersion>& artifacts,
        const std::string& timestamp) const;

private:
    SemanticEventFactory semantic_event_factory_;
};

} // namespace arcs::core::commit
