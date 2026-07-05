/**
 * @file ingress_source.hpp
 * @brief Defines the raw IngressEvent type and the IIngressSource
 *        interface that all sources of incoming data (CLI, file, API,
 *        etc.) must implement before events are normalized.
 */
#pragma once

#include <string>

#include "artifact/artifact.hpp"

namespace arcs::ingress {

// Raw-Ingress-Event – noch nicht normalisiert, noch kein validiertes Artefakt.
/**
 * @brief A raw, not-yet-normalized event captured from an ingress source.
 *        This is the pre-artifact form of incoming data; it is converted
 *        into an ArtifactVersion (type "ingress_event") by a normalizer.
 */
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
/**
 * @brief Interface for a source that supplies raw IngressEvent instances
 *        into the ingestion pipeline (e.g. CLI stdin, file watcher, API
 *        endpoint).
 */
class IIngressSource {
public:
    virtual ~IIngressSource() = default;

    // Liest ein Event von der Qülle.
    // Kann blocking sein (z.B. CLI-Eingabe) oder sofort returnen (z.B. API-Payload).
    /**
     * @brief Reads the next event from the source. May block (e.g. CLI
     *        input) or return immediately (e.g. an already-buffered API
     *        payload).
     * @return The next IngressEvent produced by this source.
     */
    virtual IngressEvent emit() = 0;

    // Ob die Qülle weitere Events liefert.
    /**
     * @brief Indicates whether the source may still produce further events.
     * @return True if more events are expected from this source.
     */
    virtual bool has_more() const = 0;
};

} // namespace arcs::ingress
