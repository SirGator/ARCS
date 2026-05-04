#pragma once

#include <string>

#include "interpretation/interpretation_proposal.hpp"

namespace arcs::interpretation {

class IInputInterpreter {
public:
    virtual ~IInputInterpreter() = default;

    // Converts raw user text into a non-authoritative interpretation proposal.
    //
    // Important:
    // - This does not create a final task.
    // - This does not approve anything.
    // - This does not create actions.
    // - The result must be treated as a proposal only.
    virtual InterpretationProposal interpret(const std::string& raw_text) = 0;
};

} // namespace arcs::interpretation