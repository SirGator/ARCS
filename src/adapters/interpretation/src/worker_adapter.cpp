#include "interpretation/worker_adapter.hpp"

namespace arcs::adapters::interpretation {

WorkerInterpretationAdapter::WorkerInterpretationAdapter(arcs::interpretation::InterpretationApiConfig config)
    : client_(std::move(config))
{
}

AdapterInfo WorkerInterpretationAdapter::info() const
{
    return AdapterInfo{
        .id = "worker_interpretation",
        .kind = AdapterKind::Interpretation,
        .capabilities = {"interpretation"},
    };
}

AdapterHealth WorkerInterpretationAdapter::health() const
{
    return AdapterHealth{};
}

arcs::interpretation::InterpretationResponse WorkerInterpretationAdapter::interpret(
    const arcs::interpretation::InterpretationRequest& request)
{
    return client_.interpret(request);
}

} // namespace arcs::adapters::interpretation
