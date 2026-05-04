#include <iostream>
#include <fstream>
#include <cstdlib>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "core/flow.hpp"
#include "execution/cli_output_adapter.hpp"

namespace {

struct AppConfig {
    std::optional<std::string> interpretation_api_url;
    std::optional<std::string> interpretation_api_key;
    std::optional<std::string> interpretation_model;
    std::optional<std::string> interpretation_system_prompt;
};

void set_env(const char* name, const std::optional<std::string>& value)
{
    if (!value.has_value() || value->empty()) {
        return;
    }

#if defined(_WIN32)
    _putenv_s(name, value->c_str());
#else
    setenv(name, value->c_str(), 1);
#endif
}

std::string trim(std::string value)
{
    const auto first = value.find_first_not_of(" \t\r\n");
    if (first == std::string::npos) {
        return {};
    }

    const auto last = value.find_last_not_of(" \t\r\n");
    return value.substr(first, last - first + 1);
}

std::optional<std::string> strip_quotes(std::string value)
{
    value = trim(std::move(value));
    if (value.size() >= 2) {
        const char first = value.front();
        const char last = value.back();
        if ((first == '"' && last == '"') || (first == '\'' && last == '\'')) {
            return value.substr(1, value.size() - 2);
        }
    }

    return value;
}

std::optional<AppConfig> load_yaml_config(const std::filesystem::path& path)
{
    std::ifstream in(path);
    if (!in) {
        return std::nullopt;
    }

    AppConfig config;
    std::string line;
    while (std::getline(in, line)) {
        line = trim(std::move(line));
        if (line.empty() || line.front() == '#') {
            continue;
        }

        const auto colon = line.find(':');
        if (colon == std::string::npos) {
            continue;
        }

        const auto key = trim(line.substr(0, colon));
        const auto value = strip_quotes(line.substr(colon + 1));
        if (!value.has_value()) {
            continue;
        }

        if (key == "interpretation_api_url" || key == "interpreter_api_url") {
            config.interpretation_api_url = *value;
        } else if (key == "interpretation_api_key") {
            config.interpretation_api_key = *value;
        } else if (key == "interpretation_model") {
            config.interpretation_model = *value;
        } else if (key == "interpretation_system_prompt") {
            config.interpretation_system_prompt = *value;
        }
    }

    return config;
}

std::optional<AppConfig> load_config_file(const std::filesystem::path& path)
{
    if (!std::filesystem::exists(path)) {
        return std::nullopt;
    }

    return load_yaml_config(path);
}

void apply_config(const AppConfig& config)
{
    set_env("ARCS_INTERPRETATION_API_URL", config.interpretation_api_url);
    set_env("ARCS_INTERPRETER_API_URL", config.interpretation_api_url);
    set_env("ARCS_INTERPRETATION_API_KEY", config.interpretation_api_key);
    set_env("ARCS_INTERPRETATION_MODEL", config.interpretation_model);
    set_env("ARCS_INTERPRETATION_SYSTEM_PROMPT", config.interpretation_system_prompt);
}

std::optional<std::string> current_interpretation_api_url(const AppConfig& config)
{
    if (config.interpretation_api_url.has_value() && !config.interpretation_api_url->empty()) {
        return config.interpretation_api_url;
    }

    if (const char* env = std::getenv("ARCS_INTERPRETATION_API_URL"); env != nullptr && *env != '\0') {
        return std::string(env);
    }

    if (const char* env = std::getenv("ARCS_INTERPRETER_API_URL"); env != nullptr && *env != '\0') {
        return std::string(env);
    }

    return std::nullopt;
}

void print_usage()
{
    std::cout << "Usage: arcs_app [options]\n"
              << "  --config <file>                       Load YAML config\n"
              << "  --interpretation-api-url <url>        Set LLM API endpoint\n"
              << "  --interpretation-api-key <key>        Set API key\n"
              << "  --interpretation-model <model>        Set model name\n"
              << "  --interpretation-system-prompt <txt>  Set system prompt\n";
}

AppConfig parse_args(int argc, char* argv[], bool& show_help)
{
    AppConfig config;
    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];

        if (arg == "--help" || arg == "-h") {
            show_help = true;
            continue;
        }

        auto take_value = [&](std::optional<std::string>& target) {
            if (i + 1 < argc) {
                target = argv[++i];
            }
        };

        if (arg == "--config") {
            if (i + 1 < argc) {
                const auto file_config = load_config_file(argv[++i]);
                if (file_config.has_value()) {
                    config = *file_config;
                }
            }
        } else if (arg == "--interpretation-api-url") {
            take_value(config.interpretation_api_url);
        } else if (arg == "--interpretation-api-key") {
            take_value(config.interpretation_api_key);
        } else if (arg == "--interpretation-model") {
            take_value(config.interpretation_model);
        } else if (arg == "--interpretation-system-prompt") {
            take_value(config.interpretation_system_prompt);
        }
    }

    return config;
}

} // namespace

int main(int argc, char* argv[])
{
    bool show_help = false;
    auto config = parse_args(argc, argv, show_help);

    if (show_help) {
        print_usage();
        return 0;
    }

    if (const auto default_config = load_config_file("arcs.yaml"); default_config.has_value()) {
        if (!config.interpretation_api_url.has_value()) {
            config.interpretation_api_url = default_config->interpretation_api_url;
        }
        if (!config.interpretation_api_key.has_value()) {
            config.interpretation_api_key = default_config->interpretation_api_key;
        }
        if (!config.interpretation_model.has_value()) {
            config.interpretation_model = default_config->interpretation_model;
        }
        if (!config.interpretation_system_prompt.has_value()) {
            config.interpretation_system_prompt = default_config->interpretation_system_prompt;
        }
    }

    apply_config(config);
    const auto api_url = current_interpretation_api_url(config);

    std::cout << "ARCS CLI\n";
    if (api_url.has_value()) {
        std::cout << "Interpretation API: using " << *api_url << '\n';
    } else {
        std::cout << "Interpretation API: using rule-based fallback\n";
    }
    std::cout << "Enter input like: approval=yes permission=yes\n";
    std::cout << "> " << std::flush;

    std::string line;
    std::getline(std::cin, line);

    const auto output = arcs::core::run_text_flow(line);

    arcs::execution::CliTextOutputAdapter output_adapter;
    output_adapter.write(std::cout, output);

    std::ofstream log_file("arcs.log", std::ios::app);
    if (log_file) {
        log_file << "--- ARCS run ---\n";
        log_file << "input: " << line << '\n';
        log_file << output;
        if (!output.empty() && output.back() != '\n') {
            log_file << '\n';
        }
        log_file << '\n';
    }
    return 0;
}
