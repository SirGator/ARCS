#pragma once

#include <optional>
#include <string>

namespace arcs::interpretation {

// Minimale Konfiguration fuer den externen Interpretation-Worker.
// ARCS braucht nur noch genau einen Endpunkt.
struct InterpretationApiConfig {
    // Vollstaendige URL zu POST /interpret.
    std::optional<std::string> interpret_api_url;
};

} // namespace arcs::interpretation
