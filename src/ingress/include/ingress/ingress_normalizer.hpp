/**
 * @file ingress_normalizer.hpp
 * @brief Defines the normalization stage of the ingress pipeline, which
 *        converts raw IngressEvent data into a validated "ingress_event"
 *        ArtifactVersion.
 */
#pragma once

#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "ingress/ingress_source.hpp"

namespace arcs::ingress {

// Ergebnis der Normalisierung.
/**
 * @brief Outcome status of running a raw IngressEvent through a normalizer.
 */
enum class NormalizerStatus {
    Ok,
    EmptyInput,
    InvalidEncoding,
    Truncated,
};

/**
 * @brief Result of normalizing a raw IngressEvent: the resulting
 *        "ingress_event" artifact (when successful) plus status and
 *        rejection details (when not).
 */
struct NormalizedIngress {
    NormalizerStatus status{NormalizerStatus::Ok};
    arcs::artifact::ArtifactVersion artifact;       // type = "ingress_event"
    std::string rejection_reason;   // gesetzt wenn status != Ok
};

// Interface: Raw Input → ingress_event Artefakt.
/**
 * @brief Interface for converting a raw IngressEvent into a normalized
 *        "ingress_event" artifact.
 */
class IIngressNormalizer {
public:
    virtual ~IIngressNormalizer() = default;

    /**
     * @brief Normalizes a raw ingress event into an artifact.
     * @param raw The raw event to normalize.
     * @return The normalization result, including status and (on success)
     *         the resulting artifact.
     */
    virtual NormalizedIngress normalize(const IngressEvent& raw) = 0;
};

// Standard-Implementierung: Erzeugt ingress_event-Artefakt mit Metadaten.
/**
 * @brief Default IIngressNormalizer implementation. Rejects empty payloads
 *        and otherwise builds an "ingress_event" ArtifactVersion, filling
 *        in default stream key/actor type when the raw event omits them.
 */
class DefaultIngressNormalizer final : public IIngressNormalizer {
public:
    /**
     * @brief Constructs a normalizer with fallback defaults used when a
     *        raw event does not specify its own stream key or actor type.
     * @param default_stream_key Stream key to use when the raw event's is empty.
     * @param default_actor_type Actor type to use when the raw event's is empty.
     */
    explicit DefaultIngressNormalizer(
        const std::string& default_stream_key = "session:default",
        const std::string& default_actor_type = "human");

    /**
     * @brief Normalizes a raw ingress event into an "ingress_event"
     *        artifact, rejecting it if the raw payload is empty.
     * @param raw The raw event to normalize.
     * @return The normalization result.
     */
    NormalizedIngress normalize(const IngressEvent& raw) override;

private:
    std::string default_stream_key_;
    std::string default_actor_type_;
};

} // namespace arcs::ingress
