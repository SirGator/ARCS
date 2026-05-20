#include "interpretation/worker_client.hpp"

#include <array>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <optional>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>

#include <unistd.h>

#include <nlohmann/json.hpp>

namespace arcs::interpretation {

namespace {

// Quotes fuer eine Shell sicher einpacken, damit curl-Parameter nicht zerlegt werden.
std::string shell_escape(std::string_view value)
{
    std::string escaped;
    escaped.reserve(value.size() + 2);
    escaped.push_back('\'');
    for (const char ch : value) {
        if (ch == '\'') {
            escaped += "'\\''";
        } else {
            escaped.push_back(ch);
        }
    }
    escaped.push_back('\'');
    return escaped;
}

std::optional<std::string> write_temp_json(const std::string& body)
{
    char path_template[] = "/tmp/arcs-interpretation-XXXXXX";
    // Temporaere Datei, weil curl hier per --data-binary@file gefuettert wird.
    const int fd = mkstemp(path_template);
    if (fd < 0) {
        return std::nullopt;
    }

    std::ofstream out(path_template, std::ios::binary | std::ios::trunc);
    if (!out) {
        ::close(fd);
        ::unlink(path_template);
        return std::nullopt;
    }

    out << body;
    out.close();
    ::close(fd);
    return std::string(path_template);
}

struct CurlResult {
    bool ok{false};
    std::string stdout_text;
    std::string error;
};

// Fuehrt den HTTP-POST ueber curl aus und sammelt die JSON-Antwort ein.
CurlResult run_curl_post(const std::string& url, const std::string& body)
{
    CurlResult result;
    const auto temp_path = write_temp_json(body);
    if (!temp_path.has_value()) {
        result.error = "failed to create temp request body";
        return result;
    }

    const std::string command =
        "curl -fsS -X POST " + shell_escape(url) +
        " -H 'Content-Type: application/json' --data-binary @" + shell_escape(*temp_path);

    std::array<char, 4096> buffer{};
    std::string stdout_text;
    if (FILE* pipe = ::popen(command.c_str(), "r"); pipe != nullptr) {
        while (fgets(buffer.data(), static_cast<int>(buffer.size()), pipe) != nullptr) {
            stdout_text.append(buffer.data());
        }
        const int status = ::pclose(pipe);
        ::unlink(temp_path->c_str());
        if (status == 0) {
            result.ok = true;
            result.stdout_text = std::move(stdout_text);
            return result;
        }

        result.error = "curl exited with status " + std::to_string(status);
        return result;
    }

    ::unlink(temp_path->c_str());
    result.error = "failed to spawn curl";
    return result;
}

nlohmann::json response_to_json(const std::string& body)
{
    return nlohmann::json::parse(body);
}

// Baut eine standardisierte Fehlerantwort fuer den Aufrufer.
InterpretationResponse make_error(std::string error)
{
    InterpretationResponse response;
    response.ok = false;
    response.error = std::move(error);
    return response;
}

// Mappt die rohe Worker-Antwort auf das ARCS-Response-Objekt.
InterpretationResponse parse_worker_response(const CurlResult& curl_result)
{
    if (!curl_result.ok) {
        return make_error(curl_result.error);
    }

    try {
        const auto json = response_to_json(curl_result.stdout_text);
        InterpretationResponse response;
        response.ok = json.value("ok", false);
        if (json.contains("request_id") && json["request_id"].is_string()) {
            response.request_id = json["request_id"].get<std::string>();
        }
        if (json.contains("schema_id") && json["schema_id"].is_string()) {
            response.schema_id = json["schema_id"].get<std::string>();
        }
        if (json.contains("payload") && !json["payload"].is_null()) {
            response.payload = json["payload"];
        }
        if (json.contains("error") && !json["error"].is_null()) {
            if (json["error"].is_string()) {
                response.error = json["error"].get<std::string>();
            } else {
                response.error = json["error"].dump();
            }
        }
        if (!response.ok && !response.error.has_value()) {
            response.error = std::string{"worker returned ok=false"};
        }
        return response;
    } catch (const std::exception& ex) {
        return make_error(std::string("invalid worker response: ") + ex.what());
    }
}

} // namespace

WorkerInterpretationClient::WorkerInterpretationClient(InterpretationApiConfig config)
    : config_(std::move(config))
{
}

// Ein Request enthaelt Text, Schema und Kontext.
// Der Worker soll daraus direkt ein Proposal erzeugen, ohne weitere ARCS-Rueckfragen.
InterpretationResponse WorkerInterpretationClient::interpret(const InterpretationRequest& request)
{
    const nlohmann::json body{
        {"request_id", request.request_id},
        {"raw_input", request.raw_input},
        {"schema_id", request.schema_id},
        {"schema", request.schema},
        {"context", request.context},
        {"prompt_config", request.prompt_config},
    };
    if (!config_.interpret_api_url.has_value() || config_.interpret_api_url->empty()) {
        return make_error("interpret_api_url not configured");
    }

    return parse_worker_response(run_curl_post(*config_.interpret_api_url, body.dump()));
}

} // namespace arcs::interpretation
