/**
 * @file ingress_router.cpp
 * @brief Implements DefaultIngressRouter, which matches an ingress_event
 *        artifact's source_kind and intent against registered handlers to
 *        decide the routing action, falling back to Quarantine.
 */
#include "ingress/ingress_router.hpp"

#include <algorithm>
#include <string>
#include <utility>

namespace arcs::ingress {

/**
 * @brief Registers a handler to be considered by route(), in the order
 *        added.
 * @param handler The handler to register (moved in).
 */
void DefaultIngressRouter::add_handler(Handler handler)
{
    handlers_.push_back(std::move(handler));
}

/**
 * @brief Finds the first registered handler whose source_kinds and
 *        intent_keywords match the given ingress_event's payload, and
 *        returns its action. A handler with no source_kinds/keywords
 *        matches anything for that criterion. Falls back to Quarantine if
 *        no handler matches.
 * @param ingress The ingress_event artifact to route.
 * @return The chosen route result.
 */
RouteResult DefaultIngressRouter::route(const arcs::artifact::ArtifactVersion& ingress)
{
    const auto& payload = ingress.payload;

    // Extrahiere source_kind und intent aus dem Payload.
    const auto source_kind = payload.value("source_kind", std::string{});

    std::string intent;
    if (payload.contains("intent")) {
        intent = payload["intent"].get<std::string>();
    }

    for (const auto& handler : handlers_) {
        // Pruefe source_kind (wenn Handler welche definiert).
        if (!handler.source_kinds.empty()) {
            const auto it = std::find(handler.source_kinds.begin(),
                                       handler.source_kinds.end(),
                                       source_kind);
            if (it == handler.source_kinds.end()) {
                continue;
            }
        }

        // Pruefe intent-keywords (wenn Handler welche definiert).
        if (!handler.intent_keywords.empty()) {
            bool match = false;
            for (const auto& keyword : handler.intent_keywords) {
                if (intent.find(keyword) != std::string::npos) {
                    match = true;
                    break;
                }
            }
            if (!match) {
                continue;
            }
        }

        return RouteResult{
            .action = handler.action,
            .reason = "matched handler: " + handler.name,
            .target_extractor = handler.name,
        };
    }

    // Kein Handler gefunden → Quarantine.
    return RouteResult{
        .action = RouteAction::Quarantine,
        .reason = "no matching handler for source_kind=" + source_kind + " intent=" + intent,
    };
}

} // namespace arcs::ingress
