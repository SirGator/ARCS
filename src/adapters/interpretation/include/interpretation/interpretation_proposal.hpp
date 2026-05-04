#pragma once

#include <string>
#include <vector>
#include <optional>

namespace arcs::interpretation {

enum class InterpretationStatus {
    Parsed,
    Unknown,
    Failed
};

struct InterpretationEntity {
    std::string name;
    std::string value;
};

struct InterpretationProposal {
    // Status of the interpretation attempt.
    // Parsed  = interpretation found a usable result
    // Unknown = input could not be mapped safely
    // Failed  = interpretation/API failed
    InterpretationStatus status = InterpretationStatus::Unknown;

    // Suggested intent, never authoritative.
    // Examples:
    // - create_report
    // - ask_question
    // - unknown
    std::string intent = "unknown";

    // What the user request appears to target.
    // Example: "letzte Prüfergebnisse"
    std::string target;

    // Suggested output format.
    // Examples:
    // - json
    // - pdf
    // - text
    // - unknown
    std::string format = "unknown";

    // Confidence from 0.0 to 1.0.
    // This is only a signal for later verification/mapping.
    double confidence = 0.0;

    // Original input text.
    std::string raw_text;

    // Optional extracted entities.
    std::vector<InterpretationEntity> entities;

    // Human/debug-readable reason.
    // Useful for logs and tests.
    std::string reason;
};

inline std::string to_string(InterpretationStatus status) {
    switch (status) {
        case InterpretationStatus::Parsed:
            return "parsed";
        case InterpretationStatus::Unknown:
            return "unknown";
        case InterpretationStatus::Failed:
            return "failed";
    }

    return "unknown";
}

inline InterpretationStatus interpretation_status_from_string(const std::string& value) {
    if (value == "parsed") {
        return InterpretationStatus::Parsed;
    }

    if (value == "failed") {
        return InterpretationStatus::Failed;
    }

    return InterpretationStatus::Unknown;
}

} // namespace arcs::interpretation
