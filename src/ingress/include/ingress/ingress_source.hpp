#pragma once

#include <string>

#include "artifact/artifact.hpp"

namespace arcs::ingress {

// Raw-Ingress-Event – noch nicht normalisiert, noch kein validiertes Artefakt.
struct IngressEvent {
    std::string source_kind;    // chat | file | api | sensor | timer | internal
    std::string source_ref;     // z.B. "cli", "/path/to/file", "https://..."
    std::string raw_payload;    // unverarbeiteter Input
    std::string stream_key;     // wird vom Normalizer gesetzt falls leer
    std::string actor_id;       // wer hat es gesendet
    std::string actor_type;     // human | system | model | executor
};

// Interface: Eine Qülle, die Ingress-Events liefert.
// Spec §14: interface IIngressSource { emit(): IngressEvent }
class IIngressSource {
public:
    virtual ~IIngressSource() = default;

    // Liest ein Event von der Qülle.
    // Kann blocking sein (z.B. CLI-Eingabe) oder sofort returnen (z.B. API-Payload).
    virtual IngressEvent emit() = 0;

    // Ob die Qülle weitere Events liefert.
    virtual bool has_more() const = 0;
};

} // namespace arcs::ingress
