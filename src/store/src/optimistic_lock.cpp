#include "store/optimistic_lock.hpp"

namespace arcs::store::optimistic_lock {

void validate_pending_version(
    const PendingVersion& pending,
    const IStore& store)
{
    const std::optional<std::string>& expected = pending.expected_head_version_id;

    // Kein expected_head_version_id gesetzt:
    // -> kein Optimistic-Lock-Check
    if (!expected.has_value()) {
        return;
    }

    const std::string& artifact_id = pending.version.artifact_id;
    const std::optional<std::string> current =
        store.current_head_version_id(artifact_id);

    // Es wurde ein bestimmter Head erwartet, aber aktuell gibt es keinen.
    if (!current.has_value()) {
        throw CommitRejectedError(
            "optimistic lock rejected: artifact '" + artifact_id +
            "' has no current head, but expected_head_version_id='" +
            *expected + "' was provided");
    }

    // Aktueller Head stimmt nicht mit erwartetem Head überein.
    if (*current != *expected) {
        throw CommitRejectedError(
            "optimistic lock rejected: artifact '" + artifact_id +
            "', expected head='" + *expected +
            "', current head='" + *current + "'");
    }
}

void validate_bundle(
    const CommitBundle& bundle,
    const IStore& store)
{
    for (const auto& pending : bundle.versions) {
        validate_pending_version(pending, store);
    }
}

} // namespace arcs::store::optimistic_lock
