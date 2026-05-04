#pragma once

#include <optional>
#include <string>

#include "interpretation/interpretation_proposal.hpp"

namespace arcs::interpretation {

// Minimal task representation for the interpretation adapter.
//
// If your project already has a real Task/Artifact builder type,
// replace this struct with that type later.
struct TaskDraft {
    std::string title;
    std::string intent;
    std::string target;
    std::string format;
    std::string source_text;
};

class InterpretationToTaskMapper {
public:
    InterpretationToTaskMapper() = default;

    // Converts a low-trust interpretation proposal into a task draft
    // only when the proposal is safe and specific enough.
    //
    // Unknown/failed/low-confidence proposals return std::nullopt.
    std::optional<TaskDraft> map(const InterpretationProposal& proposal) const;

private:
    static bool is_supported_intent(const std::string& intent);
    static bool is_supported_format(const std::string& format);
};

} // namespace arcs::interpretation