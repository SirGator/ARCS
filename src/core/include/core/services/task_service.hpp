/**
 * @file task_service.hpp
 * @brief Creates the task artifact that anchors a flow run, derived from an
 *        ingress event and its parsed input.
 */

#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "core/services/common.hpp"

namespace arcs::core::services {

/**
 * @brief Builds the task artifact representing the work item derived from
 *        an ingested input.
 */
class TaskService {
public:
    /**
     * @brief Creates a task artifact from an ingress event and its parsed
     *        input.
     * @param ingress_event Ingress event the task originates from.
     * @param input Original raw input text.
     * @param parsed_input Parsed flags extracted from the input.
     * @return The created task artifact.
     */
    arcs::artifact::ArtifactVersion create_task(
        const arcs::artifact::ArtifactVersion& ingress_event,
        const std::string& input,
        const ParsedInput& parsed_input) const;
};

} // namespace arcs::core::services
