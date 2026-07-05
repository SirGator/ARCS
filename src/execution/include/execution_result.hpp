/**
 * @file execution_result.hpp
 * @brief Legacy compatibility shim that re-exports the execution result
 *        types from arcs::execution into the arcs namespace, for callers
 *        that still include the flat "execution_result.hpp" path instead
 *        of "execution/execution_result.hpp".
 */
#pragma once

#include "execution/execution_result.hpp"

namespace arcs {

using execution::ActionRef;
using execution::ExecutionLog;
using execution::ExecutionResult;
using execution::ExecutionStatus;

} // namespace arcs
