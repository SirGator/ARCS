/**
 * @file idempotency.hpp
 * @brief Legacy compatibility shim that re-exports IIdempotencyStore from
 *        arcs::execution into the arcs namespace, for callers still
 *        including the flat "idempotency.hpp" path.
 */
#pragma once

#include "execution/idempotency.hpp"

namespace arcs {

using execution::IIdempotencyStore;

} // namespace arcs
