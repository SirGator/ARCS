#pragma once

#include <filesystem>

#include "artifact/artifact.hpp"
#include "core/flow_result.hpp"
#include "core/runtime/runtime_context.hpp"
#include "schema/schema_registry.hpp"

namespace arcs::core::runtime {

const arcs::schema::SchemaRegistry& default_payload_schema_registry();

FlowResult finalize_runtime(
    RuntimeContext& context);

FlowResult run_fresh_flow(
    RuntimeContext& context);

FlowResult resume_approval_flow(
    RuntimeContext& context,
    const arcs::artifact::ArtifactVersion& input_artifact);

} // namespace arcs::core::runtime
