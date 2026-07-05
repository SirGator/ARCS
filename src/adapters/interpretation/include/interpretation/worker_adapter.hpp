#pragma once

#include "adapters/interpretation/interpretation_adapter.hpp"
#include "interpretation/config.hpp"
#include "interpretation/worker_client.hpp"

namespace arcs::adapters::interpretation {

class WorkerInterpretationAdapter final : public IInterpretationAdapter {
public:
    explicit WorkerInterpretationAdapter(arcs::interpretation::InterpretationApiConfig config);

    AdapterInfo info() const override;
    AdapterHealth health() const override;
    arcs::interpretation::InterpretationResponse interpret(
        const arcs::interpretation::InterpretationRequest& request) override;

private:
    arcs::interpretation::WorkerInterpretationClient client_;
};

} // namespace arcs::adapters::interpretation
