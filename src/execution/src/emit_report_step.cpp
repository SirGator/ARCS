/**
 * @file emit_report_step.cpp
 * @brief Converts an EmitReportStep's parameters into the JSON action
 *        parameter shape, validating format and sections.
 */
#include "step.hpp"
#include <nlohmann/json.hpp>
#include <stdexcept>

namespace arcs::execution {

/**
 * @brief Validate and convert an EmitReportStep's parameters into the
 *        JSON representation used in an action's payload.
 * @param step Step whose params (format, sections) are converted.
 * @return JSON object with "format" and "sections" fields.
 * @throws std::runtime_error if format is not "pdf"/"json", or sections
 *         is empty.
 */
nlohmann::json emit_report_params_to_action_params(const EmitReportStep& step) {
    if (step.params.format != "pdf" && step.params.format != "json") {
        throw std::runtime_error("emit_report: unsupported format");
    }

    if (step.params.sections.empty()) {
        throw std::runtime_error("emit_report: sections must not be empty");
    }

    return {
        {"format", step.params.format},
        {"sections", step.params.sections}
    };
}

} // namespace arcs::execution
