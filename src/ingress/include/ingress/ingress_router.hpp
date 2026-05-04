#pragma once

#include <memory>
#include <string>
#include <vector>

#include "artifact/artifact.hpp"

namespace arcs::ingress {

// Entscheidet, welcher Handler/Extraktor für ein ingress_event zuständig ist.
enum class RouteAction {
    ExtractToTask,       // NLU/Extraktor: ingress_event → task
    ExtractToClaim,      // ingress_event → claim
    ExtractToEvidence,   // ingress_event → evidence
    PassThrough,         // Bereits typisiert, durchreichen
    Quarantine,          // Kein Handler zuständig
};

struct RouteResult {
    RouteAction action{RouteAction::Quarantine};
    std::string reason;
    std::string target_extractor;  // leer wenn nicht relevant
};

// Interface: Bestimmt die Route für ein ingress_event.
class IIngressRouter {
public:
    virtual ~IIngressRouter() = default;

    virtual RouteResult route(const arcs::artifact::ArtifactVersion& ingress) = 0;
};

// Regelbasierter Router: entscheidet nach source_kind und payload-Inhalt.
class DefaultIngressRouter final : public IIngressRouter {
public:
    // Handler registrieren, die bestimmte source_kinds oder intent-Pattern bedienen.
    struct Handler {
        std::string name;
        std::vector<std::string> source_kinds;  // leer = alle
        std::vector<std::string> intent_keywords; // leer = alle
        RouteAction action;
    };

    void add_handler(Handler handler);

    RouteResult route(const arcs::artifact::ArtifactVersion& ingress) override;

private:
    std::vector<Handler> handlers_;
};

} // namespace arcs::ingress
