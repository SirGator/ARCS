/**
 * @file revocation_check.hpp
 * @brief Legacy compatibility shim that re-exports is_revoked() from
 *        arcs::execution into the arcs namespace, for callers still
 *        including the flat "revocation_check.hpp" path.
 */
#pragma once

#include "execution/revocation_check.hpp"

namespace arcs {

using execution::is_revoked;

} // namespace arcs
