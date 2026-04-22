#include "step.hpp"
#include <nlohmann/json.hpp>
#include <stdexcept>

namespace arcs::execution {

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
