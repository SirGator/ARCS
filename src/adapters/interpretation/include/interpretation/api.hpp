#pragma once

#include "interpretation/config.hpp"
#include "interpretation/request.hpp"
#include "interpretation/response.hpp"

namespace arcs::interpretation {

// Ein einzelner Vertrag fuer Interpretationsanfragen.
// Der Client schickt einen Auftrag hinein und bekommt genau ein Proposal zurueck.
class IInterpretationApi {
public:
    virtual ~IInterpretationApi() = default;

    // Fuehrt die Interpretation aus und liefert das strukturierte Ergebnis.
    virtual InterpretationResponse interpret(const InterpretationRequest& request) = 0;
};

} // namespace arcs::interpretation
