/**
 * @file materializer.hpp
 * @brief Declares the interface and concrete implementation for turning
 *        an option artifact (given a bound policy) into a set of
 *        executable action artifacts.
 */
#pragma once

#include <vector>

namespace arcs::artifact {
struct ArtifactVersion;
}

namespace arcs::execution {

/// An option artifact version being materialized into actions.
using OptionArtifact = arcs::artifact::ArtifactVersion;
/// A policy artifact version the option is validated/bound against.
using PolicyArtifact = arcs::artifact::ArtifactVersion;
/// An action-candidate artifact version produced by materialization.
using ActionArtifact = arcs::artifact::ArtifactVersion;

/**
 * @brief Interface for converting an option artifact into concrete action
 *        artifacts, subject to the constraints of a bound policy.
 */
class IMaterializer {
public:
    virtual ~IMaterializer() = default;

    /**
     * @brief Materialize the steps described by an option into action
     *        artifacts, validated against the given policy.
     * @param option Option artifact whose steps are to be materialized.
     * @param policy Policy artifact the option must be bound to and that
     *               constrains which actions are permitted.
     * @return The list of action artifacts derived from the option.
     */
    virtual std::vector<ActionArtifact>
    materialize(const OptionArtifact& option,
                const PolicyArtifact& policy) const = 0;
};

/**
 * @brief Default IMaterializer implementation: parses an option's steps
 *        (currently emit_report), validates policy binding and
 *        capabilities, and produces deterministic action-candidate
 *        artifacts.
 */
class ActionMaterializer final : public IMaterializer {
public:
    /**
     * @brief Materialize the steps described by an option into action
     *        artifacts, validated against the given policy.
     * @param option Option artifact whose steps are to be materialized.
     * @param policy Policy artifact the option must be bound to and that
     *               constrains which actions are permitted.
     * @return The list of action artifacts derived from the option.
     */
    std::vector<ActionArtifact>
    materialize(const OptionArtifact& option,
                const PolicyArtifact& policy) const override;
};

} // namespace arcs::execution
