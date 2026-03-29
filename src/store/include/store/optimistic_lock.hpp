#pragma once

#include "store/commit.hpp"
#include "store/store.hpp"


namespace arcs::store::optimistic_lock {

using arcs::store::CommitBundle;
using arcs::store::CommitRejectedError;
using arcs::store::IStore;
using arcs::store::PendingVersion;

// Prüft genau eine PendingVersion gegen den aktuellen Head im Store.
//
// Regeln:
// - expected_head_version_id nicht gesetzt:
//     -> kein Lock-Check
// - expected_head_version_id gesetzt:
//     -> aktueller Head muss existieren
//     -> aktueller Head muss exakt expected_head_version_id entsprechen
//
// Bei Verstoß wird CommitRejectedError geworfen.
void validate_pending_version(
    const PendingVersion& pending,
    const IStore& store);

// Prüft alle PendingVersion-Einträge eines CommitBundles.
//
// Bei der ersten Verletzung wird CommitRejectedError geworfen.
void validate_bundle(
    const CommitBundle& bundle,
    const IStore& store);

} // namespace arcs::store::optimistic_lock
