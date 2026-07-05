/**
 * @file planning_service.hpp
 * @brief Turns a task into a concrete "report" option under a given policy.
 */

#pragma once

#include <string>

#include "artifact/artifact.hpp"

namespace arcs::core::services {

/**
 * @brief Plans candidate options for a task, currently limited to producing
 *        a report option.
 */
class PlanningService {
public:
    /**
     * @brief Creates a report option for the given task under the given
     *        policy reference.
     * @param task_artifact Task the option is planned for.
     * @param policy_ref Policy artifact the option must respect.
     * @param input Original input text the task was derived from.
     * @return The created report option artifact.
     */
    arcs::artifact::ArtifactVersion create_report_option(
        const arcs::artifact::ArtifactVersion& task_artifact,
        const arcs::artifact::ArtifactVersion& policy_ref,
        const std::string& input) const;
};

} // namespace arcs::core::services
