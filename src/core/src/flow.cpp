/**
 * @file flow.cpp
 * @brief Top-level entry points that run the core text/artifact flow end to
 * end and render the result to a human-readable string.
 *
 * These functions are thin wrappers that delegate the actual pipeline
 * execution to CoreRuntime and rendering to RunArtifactsService.
 */
#include "core/flow.hpp"

#include "core/runtime/core_request.hpp"
#include "core/runtime/core_runtime.hpp"
#include "core/services/run_artifacts_service.hpp"

namespace arcs::core {

/**
 * @brief Runs the core flow for a raw text input and renders the result.
 * @param input Raw text input to process.
 * @param interpretation_config Optional interpretation API configuration;
 *        may be null to use defaults.
 * @param options Flow execution options.
 * @return Human-readable rendering of the flow result.
 */
std::string run_text_flow(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config,
    const FlowOptions& options)
{
    return services::RunArtifactsService{}.render(runtime::CoreRuntime{}.run(runtime::CoreRequest{
        .input = input,
        .interpretation_config = interpretation_config,
        .options = options,
    }));
}

/**
 * @brief Runs the core flow starting from an already-constructed input
 * artifact and renders the result.
 * @param input_artifact The artifact to feed into the runtime as input.
 * @param options Flow execution options.
 * @return Human-readable rendering of the flow result.
 */
std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact, const FlowOptions& options)
{
    return services::RunArtifactsService{}.render(runtime::CoreRuntime{}.run(input_artifact, options));
}

} // namespace arcs::core
