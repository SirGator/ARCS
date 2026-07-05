/**
 * @file step.hpp
 * @brief Defines the step types that an option artifact's plan can be
 *        made of (emit_report, write_file, api_call) and the Step variant
 *        that unifies them. Only EmitReportStep is currently materialized
 *        by ActionMaterializer; the others are reserved for future use.
 */
#pragma once

#include <string>
#include <variant>
#include <vector>

namespace arcs::execution {

/** @brief Parameters for an emit_report step. */
struct EmitReportParams {
    std::string format;                 // "pdf" | "json"
    std::vector<std::string> sections;  // z.B. "summary", "risks"
};

/** @brief Step that requests generation of a report in a given format. */
struct EmitReportStep {
    static constexpr const char* kind = "emit_report";
    EmitReportParams params;
};

/** @brief Parameters for a write_file step. */
struct WriteFileParams {
    std::string path;
    std::string content_artifact_id;
};

/** @brief Step that requests writing artifact content to a file path. */
struct WriteFileStep {
    static constexpr const char* kind = "write_file";
    WriteFileParams params;
};

/** @brief Parameters for an api_call step. */
struct ApiCallParams {
    std::string endpoint;
    std::string method;
    std::string body_artifact_id;
};

/** @brief Step that requests an outbound API call. */
struct ApiCallStep {
    static constexpr const char* kind = "api_call";
    ApiCallParams params;
};

// V1/MVP: erstmal klein halten.
// Wenn du maximal nah an deinem Plan bleiben willst,
// reicht anfangs sogar nur EmitReportStep.
/**
 * @brief Discriminated union of all step kinds that can appear in an
 *        option's plan.
 */
using Step = std::variant<
    EmitReportStep,
    WriteFileStep,
    ApiCallStep
>;

} // namespace arcs::execution
