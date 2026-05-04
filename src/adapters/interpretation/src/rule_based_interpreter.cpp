#include "interpretation/rule_based_interpreter.hpp"

#include <algorithm>
#include <cctype>
#include <string>

namespace arcs::interpretation {

InterpretationProposal RuleBasedInterpreter::interpret(const std::string& raw_text) {
    const std::string lower_text = to_lower(raw_text);

    InterpretationProposal proposal;
    proposal.raw_text = raw_text;
    proposal.intent = detect_intent(lower_text);
    proposal.format = detect_format(lower_text);

    if (proposal.intent == "unknown") {
        proposal.status = InterpretationStatus::Unknown;
        proposal.target = raw_text;
        proposal.confidence = 0.20;
        proposal.reason = "no supported intent detected";
        return proposal;
    }

    proposal.status = InterpretationStatus::Parsed;
    proposal.target = raw_text;
    proposal.confidence = 0.65;
    proposal.reason = "rule based interpretation";

    return proposal;
}

std::string RuleBasedInterpreter::to_lower(std::string text) {
    std::transform(
        text.begin(),
        text.end(),
        text.begin(),
        [](unsigned char c) {
            return static_cast<char>(std::tolower(c));
        }
    );

    return text;
}

bool RuleBasedInterpreter::contains(const std::string& text, const std::string& needle) {
    return text.find(needle) != std::string::npos;
}

std::string RuleBasedInterpreter::detect_intent(const std::string& lower_text) {
    if (
        contains(lower_text, "report") ||
        contains(lower_text, "bericht") ||
        contains(lower_text, "zusammenfassung") ||
        contains(lower_text, "prüfergebnis") ||
        contains(lower_text, "pruefergebnis")
    ) {
        return "create_report";
    }

    if (
        contains(lower_text, "frage") ||
        contains(lower_text, "was ist") ||
        contains(lower_text, "warum") ||
        contains(lower_text, "wie ")
    ) {
        return "ask_question";
    }

    return "unknown";
}

std::string RuleBasedInterpreter::detect_format(const std::string& lower_text) {
    if (contains(lower_text, "json")) {
        return "json";
    }

    if (contains(lower_text, "pdf")) {
        return "pdf";
    }

    if (
        contains(lower_text, "text") ||
        contains(lower_text, "txt") ||
        contains(lower_text, "antwort")
    ) {
        return "text";
    }

    // Safe default for supported report/question tasks.
    return "text";
}

} // namespace arcs::interpretation