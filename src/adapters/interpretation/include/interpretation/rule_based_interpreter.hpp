#pragma once

#include <string>

#include "interpretation/input_interpreter.hpp"
#include "interpretation/interpretation_proposal.hpp"

namespace arcs::interpretation {

class RuleBasedInterpreter final : public IInputInterpreter {
public:
    RuleBasedInterpreter() = default;

    InterpretationProposal interpret(const std::string& raw_text) override;

private:
    static std::string to_lower(std::string text);
    static bool contains(const std::string& text, const std::string& needle);

    static std::string detect_intent(const std::string& lower_text);
    static std::string detect_format(const std::string& lower_text);
};

} // namespace arcs::interpretation