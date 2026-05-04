#include "ingress/ingress_router.hpp"

#include <algorithm>
#include <string>
#include <utility>

namespace arcs::ingress {

void DefaultIngressRouter::add_handler(Handler handler)
{
    handlers_.push_back(std::move(handler));
}

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
