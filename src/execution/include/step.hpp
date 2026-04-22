#pragma once

#include <string>
#include <variant>
#include <vector>

namespace arcs::execution {

struct EmitReportParams {
    std::string format;                 // "pdf" | "json"
    std::vector<std::string> sections;  // z.B. "summary", "risks"
};

struct EmitReportStep {
    static constexpr const char* kind = "emit_report";
    EmitReportParams params;
};

struct WriteFileParams {
    std::string path;
    std::string content_artifact_id;
};

struct WriteFileStep {
    static constexpr const char* kind = "write_file";
    WriteFileParams params;
};

struct ApiCallParams {
    std::string endpoint;
    std::string method;
    std::string body_artifact_id;
};

struct ApiCallStep {
    static constexpr const char* kind = "api_call";
    ApiCallParams params;
};

// V1/MVP: erstmal klein halten.
// Wenn du maximal nah an deinem Plan bleiben willst,
// reicht anfangs sogar nur EmitReportStep.
using Step = std::variant<
    EmitReportStep,
    WriteFileStep,
    ApiCallStep
>;

} // namespace arcs::execution
