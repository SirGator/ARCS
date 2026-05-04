#include "interpretation/interpretation_to_task_mapper.hpp"

#include <sstream>

namespace arcs::interpretation {
namespace {

constexpr double kMinimumConfidence = 0.50;

bool is_empty_or_unknown(const std::string& value) {
    return value.empty() || value == "unknown";
}

} // namespace

std::optional<TaskDraft> InterpretationToTaskMapper::map(
    const InterpretationProposal& proposal
) const {
    if (proposal.status != InterpretationStatus::Parsed) {
        return std::nullopt;
    }

    if (proposal.confidence < kMinimumConfidence) {
        return std::nullopt;
    }

    if (!is_supported_intent(proposal.intent)) {
        return std::nullopt;
    }

    if (!is_supported_format(proposal.format)) {
        return std::nullopt;
    }

    if (is_empty_or_unknown(proposal.target)) {
        return std::nullopt;
    }

    TaskDraft task;
    task.intent = proposal.intent;
    task.target = proposal.target;
    task.format = proposal.format;
    task.source_text = proposal.raw_text;

    std::ostringstream title;
    title << "Interpret user request";

    if (!proposal.intent.empty()) {
        title << ": " << proposal.intent;
    }

    if (!proposal.target.empty()) {
        title << " -> " << proposal.target;
    }

    task.title = title.str();

    return task;
}

bool InterpretationToTaskMapper::is_supported_intent(const std::string& intent) {
    return intent == "create_report"
        || intent == "ask_question";
}

bool InterpretationToTaskMapper::is_supported_format(const std::string& format) {
    return format == "json"
        || format == "pdf"
        || format == "text";
}

} // namespace arcs::interpretation