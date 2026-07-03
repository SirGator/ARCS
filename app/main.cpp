#include <iostream>
#include <fstream>
#include <filesystem>
#include <optional>
#include <string>
#include <utility>
#include <cstdlib>

#include "core/flow.hpp"
#include "interpretation/config.hpp"
#include "execution/cli_output_adapter.hpp"

namespace {

struct AppConfig {
    std::optional<std::string> interpret_api_url;
};

std::string trim(std::string value);

bool env_flag_enabled(const char* name)
{
    const char* value = std::getenv(name);
    if (value == nullptr) {
        return false;
    }

    const std::string normalized = trim(value);
    return normalized == "1" || normalized == "true" || normalized == "yes";
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

        if (key == "interpret_api_url") {
            config.interpret_api_url = *value;
        }
    }

    return config;
}

void print_usage()
{
    std::cout << "Usage: arcs_app [options]\n"
              << "  --config <file>                       Load YAML config\n"
              << "  --demo-control                        Allow demo approval/permission flags\n";
}

struct ParsedArgs {
    std::optional<std::filesystem::path> config_path;
    bool demo_control{false};
};

ParsedArgs parse_args(int argc, char* argv[], bool& show_help)
{
    ParsedArgs parsed;

    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];

        if (arg == "--help" || arg == "-h") {
            show_help = true;
        } else if (arg == "--config" && i + 1 < argc) {
            parsed.config_path = std::filesystem::path(argv[++i]);
        } else if (arg == "--demo-control") {
            parsed.demo_control = true;
        }
    }

    return parsed;
}

} // namespace

int main(int argc, char* argv[])
{
    bool show_help = false;
    const auto args = parse_args(argc, argv, show_help);

    if (show_help) {
        print_usage();
        return 0;
    }

    std::optional<AppConfig> loaded_config;
    if (args.config_path.has_value()) {
        loaded_config = load_yaml_config(*args.config_path);
    } else if (std::filesystem::exists("config/arcs.yaml")) {
        loaded_config = load_yaml_config("config/arcs.yaml");
    } else if (std::filesystem::exists("arcs.yaml")) {
        loaded_config = load_yaml_config("arcs.yaml");
    }

    const AppConfig config = loaded_config.value_or(AppConfig{});
    const bool demo_control_enabled = args.demo_control || env_flag_enabled("ARCS_DEMO_MODE");

    std::cout << "ARCS CLI\n";
    std::cout << "Interpret API: " << (config.interpret_api_url.value_or("<unset>")) << '\n';
    std::cout << "Demo controls: " << (demo_control_enabled ? "enabled" : "disabled") << '\n';
    std::cout << "Enter input\n";
    std::cout << "> " << std::flush;

    std::string line;
    std::getline(std::cin, line);

    arcs::interpretation::InterpretationApiConfig interpretation_config{
        .interpret_api_url = config.interpret_api_url,
    };

    const arcs::core::FlowOptions flow_options{
        .enable_demo_controls = demo_control_enabled,
    };

    const auto output = arcs::core::run_text_flow(line, &interpretation_config, flow_options);

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
