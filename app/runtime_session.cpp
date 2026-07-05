#include "runtime_session.hpp"

namespace arcs::app {

std::string RuntimeSession::run_text(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config,
    const arcs::core::FlowOptions& options)
{
    const auto result = runtime_.run(arcs::core::runtime::CoreRequest{
        .input = input,
        .interpretation_config = interpretation_config,
        .options = options,
    });
    return output_service_.render(result);
}

} // namespace arcs::app
