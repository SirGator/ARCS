/**
 * @file executor.hpp
 * @brief Legacy compatibility shim that re-exports IExecutor and
 *        ExecutionContext from arcs::execution into the arcs namespace,
 *        for callers still including the flat "executor.hpp" path.
 */
#pragma once

#include "execution/executor.hpp"

namespace arcs {

using execution::ExecutionContext;
using execution::IExecutor;

} // namespace arcs
