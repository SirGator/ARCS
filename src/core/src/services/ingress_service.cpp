/**
 * @file ingress_service.cpp
 * @brief Implements `IngressService::run`, which reads raw CLI-style input,
 *        normalizes it into an artifact, validates it, and routes it to the
 *        next stage, quarantining anything that fails normalization or
 *        validation along the way.
 */

#include "core/services/ingress_service.hpp"

#include <utility>

#include "ingress/ingress_normalizer.hpp"
#include "ingress/ingress_router.hpp"
#include "ingress/ingress_validator.hpp"

namespace arcs::core::services {

namespace {

arcs::ingress::IngressEvent to_ingress_event(const IngressDraft& draft)
{
    return arcs::ingress::IngressEvent{
        .source_kind = draft.signal.source_kind,
        .source_ref = draft.signal.source_ref,
        .raw_payload = draft.signal.raw_payload,
        .stream_key = draft.signal.stream_key,
        .actor_id = draft.signal.actor_id,
        .actor_type = draft.signal.actor_type,
    };
}

} // namespace

/**
 * @brief Ingests a raw input string: reads it via a CLI ingress source,
 *        normalizes it into an artifact (tagged with the "session:cli"
 *        stream), validates the normalized artifact, and if it passes,
 *        routes it using a router configured with a "chat"-source task
 *        extractor and an "internal"-source passthrough handler. Any
 *        failure at the source, normalize, or validate stage causes the
 *        input to be quarantined and a failed result to be returned.
 * @param raw_input Raw text input to ingest.
 * @param quarantine Quarantine store used to record rejected input.
 * @return The ingress result: on success, the produced artifact and routing
 *         decision; on failure, the rejection reason and stage.
 */
IngressResult IngressService::run(const IngressDraft& draft, arcs::ingress::QuarantineStore& quarantine) const
{
    IngressResult result;

    const auto raw_event = to_ingress_event(draft);
    if (raw_event.raw_payload.empty()) {
        result.rejection_reason = "no input";
        result.rejection_stage = "source";
        return result;
    }

    arcs::ingress::DefaultIngressNormalizer normalizer(draft.signal.stream_key, draft.signal.actor_type);
    auto normalized = normalizer.normalize(raw_event);

    if (normalized.status != arcs::ingress::NormalizerStatus::Ok) {
        result.rejection_reason = normalized.rejection_reason;
        result.rejection_stage = "normalize";
        arcs::ingress::QuarantinedEvent q;
        q.artifact = normalized.artifact;
        q.rejection_reason = normalized.rejection_reason;
        q.rejected_at = normalized.artifact.created_at;
        q.rejection_stage = "normalize";
        quarantine.store(std::move(q));
        return result;
    }

    arcs::ingress::MinimalIngressValidator validator;
    const auto validation = validator.validate(normalized.artifact);
    if (validation.status != arcs::ingress::ValidationStatus::Pass) {
        result.rejection_reason = validation.reason;
        result.rejection_stage = "validate";
        arcs::ingress::QuarantinedEvent q;
        q.artifact = normalized.artifact;
        q.rejection_reason = validation.reason;
        q.rejected_at = normalized.artifact.created_at;
        q.rejection_stage = "validate";
        quarantine.store(std::move(q));
        return result;
    }

    arcs::ingress::DefaultIngressRouter router;
    router.add_handler(arcs::ingress::DefaultIngressRouter::Handler{
        .name = "nlu_task_extractor",
        .source_kinds = {"chat"},
        .intent_keywords = {},
        .action = arcs::ingress::RouteAction::ExtractToTask,
    });
    router.add_handler(arcs::ingress::DefaultIngressRouter::Handler{
        .name = "passthrough",
        .source_kinds = {"internal"},
        .intent_keywords = {},
        .action = arcs::ingress::RouteAction::PassThrough,
    });

    const auto route = router.route(normalized.artifact);
    result.success = true;
    result.ingress_artifact = std::move(normalized.artifact);
    result.route_action = route.action;
    return result;
}

IngressResult IngressService::run(const std::string& raw_input, arcs::ingress::QuarantineStore& quarantine) const
{
    return run(IngressDraft{.signal = ExternalSignal{.raw_payload = raw_input}}, quarantine);
}

} // namespace arcs::core::services
