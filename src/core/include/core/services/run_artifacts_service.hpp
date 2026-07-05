/**
 * @file run_artifacts_service.hpp
 * @brief Persists a flow run's committed artifacts and quarantine contents
 *        to disk, and renders a human-readable summary of the result.
 */

#pragma once

#include <filesystem>
#include <string>

#include "artifact/artifact.hpp"
#include "core/flow_result.hpp"
#include "event/event.hpp"
#include "ingress/quarantine.hpp"
#include "store/commit.hpp"

namespace arcs::core::services {

/**
 * @brief Writes a flow run's outputs (commit bundle, quarantine, input,
 *        output) to a per-run directory on disk, and renders flow results
 *        as text.
 */
class RunArtifactsService {
public:
    /**
     * @brief Creates a fresh directory to hold this run's artifacts.
     * @return Path to the created run artifacts directory.
     */
    std::filesystem::path make_run_artifacts_dir() const;

    /**
     * @brief Persists a run's commit bundle, quarantine contents, input, and
     *        output into the given directory.
     * @param run_dir Directory to write the run's artifacts into.
     * @param bundle Commit bundle produced by the run.
     * @param quarantine Quarantine store holding any rejected input.
     * @param input Original input text for the run.
     * @param output Rendered output text for the run.
     */
    void persist(
        const std::filesystem::path& run_dir,
        const arcs::store::commit::CommitBundle& bundle,
        const arcs::ingress::QuarantineStore& quarantine,
        const std::string& input,
        const std::string& output) const;

    /**
     * @brief Renders a flow result into a human-readable text summary.
     * @param result Flow result to render.
     * @return The rendered summary text.
     */
    std::string render(const arcs::core::FlowResult& result) const;
};

} // namespace arcs::core::services
