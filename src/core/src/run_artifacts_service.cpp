/**
 * @file run_artifacts_service.cpp
 * @brief Implements RunArtifactsService, which persists a flow run's
 * artifacts, events, and quarantined items to disk and renders a
 * human-readable summary of a FlowResult.
 */
#include "core/services/run_artifacts_service.hpp"

#include <chrono>
#include <cstdlib>
#include <ctime>
#include <fstream>
#include <iomanip>
#include <sstream>
#include <system_error>

#include <nlohmann/json.hpp>

#include "artifact/json.hpp"
#include "event/json.hpp"

namespace arcs::core::services {
namespace {

std::string render_step_diagnostic(const arcs::core::Diagnostic& diagnostic)
{
    if (diagnostic.stage.empty()) {
        return diagnostic.message;
    }

    std::ostringstream out;
    out << "step: " << diagnostic.stage << " -> "
        << (diagnostic.severity == arcs::core::DiagnosticSeverity::Error ? "FAIL" : "OK");
    if (!diagnostic.message.empty()) {
        out << " | " << diagnostic.message;
    }
    return out.str();
}

/**
 * @brief Determines the base directory under which per-run artifact
 * folders are created, honoring the ARCS_ARTIFACT_DIR environment
 * variable override.
 * @return Base directory path, defaulting to "artifacts".
 */
std::filesystem::path artifacts_base_dir()
{
    if (const char* env = std::getenv("ARCS_ARTIFACT_DIR"); env != nullptr && *env != '\0') {
        return std::filesystem::path(env);
    }
    return std::filesystem::path("artifacts");
}

/**
 * @brief Builds a filesystem-safe timestamp string for the current instant,
 * used to name per-run artifact directories.
 * @return Timestamp formatted as "YYYY-MM-DD_HH-MM-SS-mmm".
 */
std::string run_timestamp()
{
    const auto now = std::chrono::system_clock::now();
    const auto now_time_t = std::chrono::system_clock::to_time_t(now);
    std::tm tm{};

#if defined(_WIN32)
    localtime_s(&tm, &now_time_t);
#else
    localtime_r(&now_time_t, &tm);
#endif

    const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(
        now.time_since_epoch()) % 1000;

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%d_%H-%M-%S")
        << '-' << std::setw(3) << std::setfill('0') << millis.count();
    return out.str();
}

/**
 * @brief Sanitizes a string for use as a filename component, replacing any
 * character that isn't alphanumeric, '-', or '_' with '_'.
 * @param value The string to sanitize (taken by value, modified in place).
 * @return The sanitized string, or "unknown" if the input was empty.
 */
std::string safe_filename_component(std::string value)
{
    for (char& ch : value) {
        if (!std::isalnum(static_cast<unsigned char>(ch)) && ch != '-' && ch != '_') {
            ch = '_';
        }
    }
    if (value.empty()) {
        return "unknown";
    }
    return value;
}

/**
 * @brief Writes a JSON value to a file, pretty-printed with 2-space
 * indentation. Silently does nothing if the file cannot be opened.
 * @param path Destination file path.
 * @param value JSON value to write.
 */
void write_json_file(const std::filesystem::path& path, const nlohmann::json& value)
{
    std::ofstream out(path);
    if (!out) {
        return;
    }
    out << value.dump(2) << '\n';
}

/**
 * @brief Writes plain text to a file, ensuring the output ends with a
 * trailing newline. Silently does nothing if the file cannot be opened.
 * @param path Destination file path.
 * @param text Text content to write.
 */
void write_text_file(const std::filesystem::path& path, const std::string& text)
{
    std::ofstream out(path);
    if (!out) {
        return;
    }
    out << text;
    if (!text.empty() && text.back() != '\n') {
        out << '\n';
    }
}

} // namespace

/**
 * @brief Creates a fresh, timestamped run directory (with "artifacts",
 * "events", and "quarantine" subdirectories) under the configured base
 * artifacts directory.
 * @return Path to the newly created run directory.
 */
std::filesystem::path RunArtifactsService::make_run_artifacts_dir() const
{
    const auto run_dir = artifacts_base_dir() / run_timestamp();
    std::error_code ec;
    std::filesystem::create_directories(run_dir / "artifacts", ec);
    std::filesystem::create_directories(run_dir / "events", ec);
    std::filesystem::create_directories(run_dir / "quarantine", ec);
    return run_dir;
}

/**
 * @brief Persists a full run's outputs to disk: input/output text, each
 * committed artifact version, each committed event, and each quarantined
 * artifact, plus a manifest.json summarizing everything written.
 * @param run_dir Run directory previously created by make_run_artifacts_dir().
 * @param bundle Committed artifact versions and events for this run.
 * @param quarantine Store of artifacts rejected during this run.
 * @param input Raw input text for the run.
 * @param output Rendered output text for the run.
 */
void RunArtifactsService::persist(
    const std::filesystem::path& run_dir,
    const arcs::store::commit::CommitBundle& bundle,
    const arcs::ingress::QuarantineStore& quarantine,
    const std::string& input,
    const std::string& output) const
{
    nlohmann::json manifest;
    manifest["input"] = input;
    manifest["artifacts"] = nlohmann::json::array();
    manifest["events"] = nlohmann::json::array();
    manifest["quarantine"] = nlohmann::json::array();

    write_text_file(run_dir / "input.txt", input);
    write_text_file(run_dir / "output.txt", output);

    for (std::size_t index = 0; index < bundle.versions.size(); ++index) {
        const auto& pending = bundle.versions[index];
        const auto& version = pending.version;
        const auto filename = std::to_string(index + 1) + "_" +
            safe_filename_component(version.type) + "_" +
            safe_filename_component(version.artifact_id) + "_" +
            safe_filename_component(version.version_id) + ".json";

        write_json_file(run_dir / "artifacts" / filename, version);
        manifest["artifacts"].push_back({
            {"file", (std::filesystem::path("artifacts") / filename).string()},
            {"artifact_id", version.artifact_id},
            {"version_id", version.version_id},
            {"type", version.type},
            {"schema_id", version.schema_id},
        });
    }

    for (std::size_t index = 0; index < bundle.events.size(); ++index) {
        const auto& event = bundle.events[index];
        const auto filename = std::to_string(index + 1) + "_" +
            safe_filename_component(event.event_type) + "_" +
            safe_filename_component(event.event_id) + ".json";

        write_json_file(run_dir / "events" / filename, event);
        manifest["events"].push_back({
            {"file", (std::filesystem::path("events") / filename).string()},
            {"event_id", event.event_id},
            {"event_type", event.event_type},
            {"stream_key", event.stream_key},
        });
    }

    for (std::size_t index = 0; index < quarantine.events().size(); ++index) {
        const auto& quarantined = quarantine.events()[index];
        const auto filename = std::to_string(index + 1) + "_" +
            safe_filename_component(quarantined.artifact.type) + "_" +
            safe_filename_component(quarantined.artifact.artifact_id) + "_" +
            safe_filename_component(quarantined.artifact.version_id) + ".json";

        write_json_file(
            run_dir / "quarantine" / filename,
            nlohmann::json{
                {"artifact", quarantined.artifact},
                {"rejection_reason", quarantined.rejection_reason},
                {"rejected_at", quarantined.rejected_at},
                {"rejection_stage", quarantined.rejection_stage},
            });

        manifest["quarantine"].push_back({
            {"file", (std::filesystem::path("quarantine") / filename).string()},
            {"artifact_id", quarantined.artifact.artifact_id},
            {"version_id", quarantined.artifact.version_id},
            {"rejection_reason", quarantined.rejection_reason},
            {"rejection_stage", quarantined.rejection_stage},
        });
    }

    write_json_file(run_dir / "manifest.json", manifest);
}

/**
 * @brief Renders a FlowResult into a human-readable multi-line summary.
 * @param result The flow result to render.
 * @return The rendered summary text.
 */
std::string RunArtifactsService::render(const arcs::core::FlowResult& result) const
{
    std::ostringstream output;
    output << "input: " << result.input << '\n';
    for (const auto& diagnostic : result.diagnostics) {
        if (diagnostic.code == "step.ok" || diagnostic.code == "step.fail") {
            output << render_step_diagnostic(diagnostic) << '\n';
            continue;
        }
        if (diagnostic.code == "artifacts.dir") {
            output << "artifacts: " << diagnostic.message << '\n';
            continue;
        }

        output << diagnostic.message;
        if (!diagnostic.message.empty() && diagnostic.message.back() != '\n') {
            output << '\n';
        }
    }
    output << "decision: " << (result.status == arcs::core::FlowStatus::Completed ? "not blocked" : "blocked") << '\n';
    output << "reason: " << result.reason << '\n';
    if (result.pending.has_value()) {
        output << "pending: " << result.pending->kind << " " << result.pending->artifact_id << '\n';
    }
    return output.str();
}

} // namespace arcs::core::services
