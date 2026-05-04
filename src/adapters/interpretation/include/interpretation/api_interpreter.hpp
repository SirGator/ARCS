#pragma once

#include <string>

#include <nlohmann/json.hpp>

#include "interpretation/input_interpreter.hpp"
#include "interpretation/interpretation_proposal.hpp"

namespace arcs::interpretation {

struct ApiInterpreterConfig {
    std::string endpoint_url;
    std::string api_key;
    std::string model;
    std::string system_prompt;
};

ApiInterpreterConfig api_interpreter_config_from_environment();

InterpretationProposal interpretation_proposal_from_response(
    const std::string& raw_text,
    const nlohmann::json& response);

class ApiInterpreter final : public IInputInterpreter {
public:
    explicit ApiInterpreter(ApiInterpreterConfig config);
    explicit ApiInterpreter(std::string endpoint_url);

    InterpretationProposal interpret(const std::string& raw_text) override;

    const std::string& endpoint_url() const;

private:
    ApiInterpreterConfig config_;

    InterpretationProposal failed_proposal(
        const std::string& raw_text,
        const std::string& reason
    ) const;
};

} // namespace arcs::interpretation
