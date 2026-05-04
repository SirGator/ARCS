#include "ingress/ingress_normalizer.hpp"

#include <chrono>
#include <ctime>
#include <string>

#include "artifact/factory.hpp"
#include "artifact/ids.hpp"

namespace arcs::ingress {

namespace {

std::string utc_now()
{
    const auto now = std::chrono::system_clock::now();
    const auto time_t = std::chrono::system_clock::to_time_t(now);
    const auto tm = std::gmtime(&time_t);

    char buf[32];
    std::strftime(buf, sizeof(buf), "%Y-%m-%dT%H:%M:%SZ", tm);
    return std::string(buf);
}

} // namespace

DefaultIngressNormalizer::DefaultIngressNormalizer(
    const std::string& default_stream_key,
    const std::string& default_actor_type)
    : default_stream_key_(default_stream_key),
      default_actor_type_(default_actor_type)
{}

NormalizedIngress DefaultIngressNormalizer::normalize(const IngressEvent& raw)
{
    NormalizedIngress result;

    if (raw.raw_payload.empty()) {
        result.status = NormalizerStatus::EmptyInput;
        result.rejection_reason = "empty raw_payload";
        return result;
    }

    const auto stream_key = raw.stream_key.empty() ? default_stream_key_ : raw.stream_key;
    const auto actor_type = raw.actor_type.empty() ? default_actor_type_ : raw.actor_type;

    auto artifact = arcs::artifact::factory::make_base_artifact(
        "ingress_event",
        "arcs.ingress_event.v1",
        stream_key,
        actor_type,
        raw.actor_id.empty() ? "unknown" : raw.actor_id,
        raw.source_kind.empty() ? "internal" : raw.source_kind,
        raw.source_ref.empty() ? "unknown" : raw.source_ref,
        "high",
        actor_type == "human" ? "human" : "system",
        utc_now());

    artifact.payload = {
        {"raw_text", raw.raw_payload},
        {"source_kind", raw.source_kind},
        {"source_ref", raw.source_ref},
        {"actor_id", raw.actor_id},
    };

    artifact.provenance.rules_applied = {"ingress_normalized"};
    artifact.provenance.transform = "normalize_ingress";

    result.artifact = std::move(artifact);
    return result;
}

} // namespace arcs::ingress
