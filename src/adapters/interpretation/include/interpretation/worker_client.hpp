#pragma once

#include "interpretation/config.hpp"
#include "interpretation/api.hpp"

namespace arcs::interpretation {

// Konkrete Client-Implementierung fuer den externen Parser/Worker.
// Intern wird ein HTTP POST mit JSON gebaut und die JSON-Antwort geparst.
class WorkerInterpretationClient final : public IInterpretationApi {
public:
    explicit WorkerInterpretationClient(InterpretationApiConfig config);

    // Sendet einen einzelnen Interpretationsauftrag an den Worker.
    InterpretationResponse interpret(const InterpretationRequest& request) override;

private:
    InterpretationApiConfig config_;
};

} // namespace arcs::interpretation
