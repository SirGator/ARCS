/**
 * @file ingress_router.hpp
 * @brief Defines the routing stage of the ingress pipeline, which decides
 *        which downstream handler/extractor should process a normalized
 *        "ingress_event" artifact (or whether it should be quarantined).
 */
#pragma once

#include <memory>
#include <string>
#include <vector>

#include "artifact/artifact.hpp"

namespace arcs::ingress {

// Entscheidet, welcher Handler/Extraktor für ein ingress_event zuständig ist.
/**
 * @brief The action to take for a routed ingress_event: extract it into a
 *        specific downstream artifact type, pass it through unchanged, or
 *        quarantine it because no handler applies.
 */
enum class RouteAction {
    ExtractToTask,       // NLU/Extraktor: ingress_event → task
    ExtractToClaim,      // ingress_event → claim
    ExtractToEvidence,   // ingress_event → evidence
    PassThrough,         // Bereits typisiert, durchreichen
    Quarantine,          // Kein Handler zuständig
};

/**
 * @brief Outcome of routing an ingress_event: the chosen action, a
 *        human-readable reason, and (if applicable) the name of the
 *        extractor/handler selected.
 */
struct RouteResult {
    RouteAction action{RouteAction::Quarantine};
    std::string reason;
    std::string target_extractor;  // leer wenn nicht relevant
};

// Interface: Bestimmt die Route für ein ingress_event.
/**
 * @brief Interface for deciding the route (handler/action) for a given
 *        ingress_event artifact.
 */
class IIngressRouter {
public:
    virtual ~IIngressRouter() = default;

    /**
     * @brief Determines the route for the given ingress_event artifact.
     * @param ingress The ingress_event artifact to route.
     * @return The chosen route result.
     */
    virtual RouteResult route(const arcs::artifact::ArtifactVersion& ingress) = 0;
};

// Regelbasierter Router: entscheidet nach source_kind und payload-Inhalt.
/**
 * @brief Rule-based IIngressRouter that matches registered handlers
 *        against an event's source_kind and intent keywords, in
 *        registration order, falling back to Quarantine if none match.
 */
class DefaultIngressRouter final : public IIngressRouter {
public:
    // Handler registrieren, die bestimmte source_kinds oder intent-Pattern bedienen.
    /**
     * @brief Describes a routing rule: which source kinds and/or intent
     *        keywords it applies to, and the action to take on a match.
     */
    struct Handler {
        std::string name;
        std::vector<std::string> source_kinds;  // leer = alle
        std::vector<std::string> intent_keywords; // leer = alle
        RouteAction action;
    };

    /**
     * @brief Registers a handler to be considered by route(), in the
     *        order added.
     * @param handler The handler to register.
     */
    void add_handler(Handler handler);

    /**
     * @brief Finds the first registered handler whose source_kinds and
     *        intent_keywords match the given ingress_event, and returns
     *        its action. Returns Quarantine if no handler matches.
     * @param ingress The ingress_event artifact to route.
     * @return The chosen route result.
     */
    RouteResult route(const arcs::artifact::ArtifactVersion& ingress) override;

private:
    std::vector<Handler> handlers_;
};

} // namespace arcs::ingress
