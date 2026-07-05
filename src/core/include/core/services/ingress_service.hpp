/**
 * @file ingress_service.hpp
 * @brief Routes raw input through ingress, producing either an accepted
 *        ingress artifact or a quarantined/rejected result.
 */

#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "ingress/ingress_router.hpp"
#include "ingress/quarantine.hpp"

namespace arcs::core::services {

struct ExternalSignal {
    std::string source_kind{"chat"};
    std::string source_ref{"cli"};
    std::string raw_payload;
    std::string stream_key{"session:cli"};
    std::string actor_id{"user:cli"};
    std::string actor_type{"human"};
};

struct IngressDraft {
    ExternalSignal signal{};
};

/**
 * @brief Outcome of routing raw input through ingress: whether it succeeded,
 *        the resulting ingress artifact, the routing decision, and, on
 *        failure, the rejection reason and stage.
 */
struct IngressResult {
    bool success{false};
    arcs::artifact::ArtifactVersion ingress_artifact;
    arcs::ingress::RouteAction route_action{arcs::ingress::RouteAction::Quarantine};
    std::string rejection_reason;
    std::string rejection_stage;
};

/**
 * @brief Runs raw text input through the ingress router, quarantining input
 *        that fails routing checks.
 */
class IngressService {
public:
    IngressResult run(const IngressDraft& draft, arcs::ingress::QuarantineStore& quarantine) const;

    /**
     * @brief Routes a raw input string through ingress.
     * @param raw_input Raw text input to ingest.
     * @param quarantine Quarantine store used to hold rejected input.
     * @return The ingress result.
     */
    IngressResult run(const std::string& raw_input, arcs::ingress::QuarantineStore& quarantine) const;
};

} // namespace arcs::core::services
