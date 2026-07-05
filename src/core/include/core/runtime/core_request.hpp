/**
 * @file core_request.hpp
 * @brief Input request type accepted by `CoreRuntime::run`.
 */

#pragma once

#include <string>

#include "core/flow.hpp"

namespace arcs::core::runtime {

/**
 * @brief Bundles the raw text input, optional interpretation configuration,
 *        and flow options for a single `CoreRuntime` run.
 */
struct CoreRequest {
    std::string input;
    const arcs::interpretation::InterpretationApiConfig* interpretation_config{nullptr};
    FlowOptions options{};
};

} // namespace arcs::core::runtime
