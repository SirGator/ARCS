#pragma once

#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "ingress/ingress_source.hpp"

namespace arcs::ingress {

// Ergebnis der Normalisierung.
enum class NormalizerStatus {
    Ok,
    EmptyInput,
    InvalidEncoding,
    Truncated,
};

struct NormalizedIngress {
    NormalizerStatus status{NormalizerStatus::Ok};
    arcs::artifact::ArtifactVersion artifact;       // type = "ingress_event"
    std::string rejection_reason;   // gesetzt wenn status != Ok
};

// Interface: Raw Input → ingress_event Artefakt.
class IIngressNormalizer {
public:
    virtual ~IIngressNormalizer() = default;

    virtual NormalizedIngress normalize(const IngressEvent& raw) = 0;
};

// Standard-Implementierung: Erzeugt ingress_event-Artefakt mit Metadaten.
class DefaultIngressNormalizer final : public IIngressNormalizer {
public:
    explicit DefaultIngressNormalizer(
        const std::string& default_stream_key = "session:default",
        const std::string& default_actor_type = "human");

    NormalizedIngress normalize(const IngressEvent& raw) override;

private:
    std::string default_stream_key_;
    std::string default_actor_type_;
};

} // namespace arcs::ingress
