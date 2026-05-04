#include "interpretation/api_interpreter.hpp"

#include <array>
#include <cstdio>
#include <cstdlib>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>

#include <nlohmann/json.hpp>

namespace arcs::interpretation {
namespace {

std::string default_system_prompt()
{
    return R"(You are an interpretation adapter in a fail-closed governance system.
Return one JSON object only. No markdown, no code fences, no prose.

The object must contain:
- status: parsed, unknown, or failed
- intent: create_report, ask_question, or unknown
- target: the concrete target or subject
- format: json, pdf, text, or unknown
- confidence: number from 0.0 to 1.0
- reason: short human-readable explanation
- entities: optional array of {"name": string, "value": string}

If the request cannot be interpreted safely, return status unknown.)";
}

std::string getenv_or_empty(const char* name)
{
    if (const char* value = std::getenv(name); value != nullptr && *value != '\0') {
        return value;
    }

    return {};
}

std::string shell_escape_single_quotes(const std::string& input)
{
    std::string escaped;
    escaped.reserve(input.size() + 8);

    for (char c : input) {
        if (c == '\'') {
            escaped += "'\\''";
        } else {
            escaped += c;
        }
    }

    return escaped;
}

std::string run_command_capture_stdout(const std::string& command)
{
    std::array<char, 256> buffer{};
    std::string result;

    FILE* pipe = popen(command.c_str(), "r");
    if (!pipe) {
        throw std::runtime_error("failed to open pipe for curl command");
    }

    while (fgets(buffer.data(), static_cast<int>(buffer.size()), pipe) != nullptr) {
        result += buffer.data();
    }

    const int exit_code = pclose(pipe);
    if (exit_code != 0) {
        throw std::runtime_error("curl command failed");
    }

    return result;
}

std::string trim_copy(std::string value)
{
    const auto first = value.find_first_not_of(" \t\r\n");
    if (first == std::string::npos) {
        return {};
    }

    const auto last = value.find_last_not_of(" \t\r\n");
    return value.substr(first, last - first + 1);
}

std::string strip_code_fences(std::string text)
{
    text = trim_copy(std::move(text));
    if (text.rfind("```", 0) != 0) {
        return text;
    }

    const auto first_newline = text.find('\n');
    if (first_newline != std::string::npos) {
        text = text.substr(first_newline + 1);
    }

    const auto closing = text.rfind("```");
    if (closing != std::string::npos) {
        text = text.substr(0, closing);
    }

    return trim_copy(std::move(text));
}

std::optional<nlohmann::json> parse_json_text(std::string text)
{
    text = strip_code_fences(std::move(text));
    if (text.empty()) {
        return std::nullopt;
    }

    const auto first_object = text.find('{');
    const auto last_object = text.rfind('}');
    if (first_object != std::string::npos && last_object != std::string::npos && last_object > first_object) {
        text = text.substr(first_object, last_object - first_object + 1);
    }

    try {
        const auto parsed = nlohmann::json::parse(text);
        if (parsed.is_object()) {
            return parsed;
        }
    } catch (const std::exception&) {
    }

    return std::nullopt;
}

std::optional<std::string> extract_message_content(const nlohmann::json& response)
{
    if (response.contains("choices") && response["choices"].is_array() && !response["choices"].empty()) {
        const auto& choice = response["choices"].front();
        if (choice.contains("message") && choice["message"].is_object()) {
            const auto& message = choice["message"];
            if (message.contains("content") && message["content"].is_string()) {
                return message["content"].get<std::string>();
            }
        }

        if (choice.contains("text") && choice["text"].is_string()) {
            return choice["text"].get<std::string>();
        }
    }

    if (response.contains("output") && response["output"].is_array()) {
        for (const auto& output_item : response["output"]) {
            if (!output_item.is_object() || !output_item.contains("content") || !output_item["content"].is_array()) {
                continue;
            }

            for (const auto& content_item : output_item["content"]) {
                if (content_item.is_object() && content_item.contains("text") && content_item["text"].is_string()) {
                    return content_item["text"].get<std::string>();
                }
            }
        }
    }

    return std::nullopt;
}

InterpretationStatus status_from_api_value(const std::string& status)
{
    if (status == "parsed") {
        return InterpretationStatus::Parsed;
    }

    if (status == "failed") {
        return InterpretationStatus::Failed;
    }

    return InterpretationStatus::Unknown;
}

InterpretationProposal proposal_from_payload(
    const std::string& raw_text,
    const nlohmann::json& payload)
{
    InterpretationProposal proposal;
    proposal.raw_text = raw_text;
    const std::string status_value = payload.contains("status")
        ? payload.value("status", "unknown")
        : (payload.contains("interpretation_status")
            ? payload.value("interpretation_status", "unknown")
            : (payload.value("intent", "unknown") == "unknown" ? "unknown" : "parsed"));

    proposal.status = status_from_api_value(status_value);
    proposal.intent = payload.value("intent", "unknown");
    proposal.target = payload.value("target", "");
    proposal.format = payload.value("format", "unknown");
    proposal.confidence = payload.value("confidence", 0.0);
    proposal.reason = payload.value("reason", "api interpretation proposal");

    if (payload.contains("raw_text") && payload["raw_text"].is_string()) {
        proposal.raw_text = payload["raw_text"].get<std::string>();
    }

    if (payload.contains("entities") && payload["entities"].is_array()) {
        for (const auto& entity_json : payload["entities"]) {
            InterpretationEntity entity;
            entity.name = entity_json.value("name", "");
            entity.value = entity_json.value("value", "");

            if (!entity.name.empty()) {
                proposal.entities.push_back(entity);
            }
        }
    }

    if (proposal.intent == "unknown") {
        proposal.status = InterpretationStatus::Unknown;
    }

    return proposal;
}

} // namespace

ApiInterpreterConfig api_interpreter_config_from_environment()
{
    ApiInterpreterConfig config;
    config.endpoint_url = getenv_or_empty("ARCS_INTERPRETATION_API_URL");
    if (config.endpoint_url.empty()) {
        config.endpoint_url = getenv_or_empty("ARCS_INTERPRETER_API_URL");
    }
    config.api_key = getenv_or_empty("ARCS_INTERPRETATION_API_KEY");
    config.model = getenv_or_empty("ARCS_INTERPRETATION_MODEL");
    config.system_prompt = getenv_or_empty("ARCS_INTERPRETATION_SYSTEM_PROMPT");

    if (config.model.empty()) {
        config.model = "gpt-4o-mini";
    }

    if (config.system_prompt.empty()) {
        config.system_prompt = default_system_prompt();
    }

    return config;
}

InterpretationProposal interpretation_proposal_from_response(
    const std::string& raw_text,
    const nlohmann::json& response)
{
    if (const auto content = extract_message_content(response)) {
        if (const auto parsed_payload = parse_json_text(*content)) {
            return proposal_from_payload(raw_text, *parsed_payload);
        }

        InterpretationProposal proposal;
        proposal.status = InterpretationStatus::Failed;
        proposal.intent = "unknown";
        proposal.target = "";
        proposal.format = "unknown";
        proposal.confidence = 0.0;
        proposal.raw_text = raw_text;
        proposal.reason = "LLM response was not valid JSON";
        return proposal;
    }

    if (response.is_object()) {
        return proposal_from_payload(raw_text, response);
    }

    InterpretationProposal proposal;
    proposal.status = InterpretationStatus::Failed;
    proposal.intent = "unknown";
    proposal.target = "";
    proposal.format = "unknown";
    proposal.confidence = 0.0;
    proposal.raw_text = raw_text;
    proposal.reason = "LLM response was empty or not an object";
    return proposal;
}

ApiInterpreter::ApiInterpreter(ApiInterpreterConfig config)
    : config_(std::move(config))
{
    if (config_.model.empty()) {
        config_.model = "gpt-4o-mini";
    }

    if (config_.system_prompt.empty()) {
        config_.system_prompt = default_system_prompt();
    }
}

ApiInterpreter::ApiInterpreter(std::string endpoint_url)
    : ApiInterpreter(ApiInterpreterConfig{
        .endpoint_url = std::move(endpoint_url),
        .api_key = {},
        .model = "gpt-4o-mini",
        .system_prompt = default_system_prompt(),
    })
{
}

const std::string& ApiInterpreter::endpoint_url() const
{
    return config_.endpoint_url;
}

InterpretationProposal ApiInterpreter::interpret(const std::string& raw_text)
{
    if (config_.endpoint_url.empty()) {
        return failed_proposal(raw_text, "missing interpretation API endpoint URL");
    }

    try {
        const nlohmann::json request_body = {
            {"model", config_.model},
            {"temperature", 0},
            {"messages", {
                {
                    {"role", "system"},
                    {"content", config_.system_prompt},
                },
                {
                    {"role", "user"},
                    {"content", raw_text},
                },
            }},
        };

        std::string command =
            "curl -sS "
            "--connect-timeout 5 "
            "--max-time 20 "
            "-X POST "
            "-H 'Content-Type: application/json' ";

        if (!config_.api_key.empty()) {
            command += "-H 'Authorization: Bearer " + shell_escape_single_quotes(config_.api_key) + "' ";
        }

        command +=
            "-d '" + shell_escape_single_quotes(request_body.dump()) + "' "
            "'" + shell_escape_single_quotes(config_.endpoint_url) + "'";

        const std::string response_text = run_command_capture_stdout(command);
        const auto response = nlohmann::json::parse(response_text);
        return interpretation_proposal_from_response(raw_text, response);
    } catch (const std::exception& e) {
        return failed_proposal(raw_text, e.what());
    }
}

InterpretationProposal ApiInterpreter::failed_proposal(
    const std::string& raw_text,
    const std::string& reason) const
{
    InterpretationProposal proposal;
    proposal.status = InterpretationStatus::Failed;
    proposal.intent = "unknown";
    proposal.target = "";
    proposal.format = "unknown";
    proposal.confidence = 0.0;
    proposal.raw_text = raw_text;
    proposal.reason = reason;
    return proposal;
}

} // namespace arcs::interpretation
