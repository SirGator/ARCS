/**
 * @file factory.hpp
 * @brief Declares helper functions for constructing new ArtifactVersion
 *        instances with consistent default fields (ids, version, timestamp).
 */
#pragma once

#include <string>

#include "artifact/artifact.hpp"

namespace arcs::artifact::factory {

/**
 * @brief Builds a new ArtifactVersion with freshly generated artifact and
 *        version identifiers, version set to 1, and the given metadata
 *        filled in. Does not populate payload or provenance fields.
 * @param type Artifact type name (e.g. "ingress_event").
 * @param schema_id Identifier of the schema the payload should conform to.
 * @param stream_key Logical stream/session the artifact belongs to.
 * @param created_by_actor_type Type of the actor creating the artifact
 *        (human | system | model | executor).
 * @param created_by_id Identifier of the actor creating the artifact.
 * @param source_kind Kind of source the data originated from.
 * @param source_ref Concrete reference to the source (path, URL, etc.).
 * @param trust_level Trust level to assign (low | medium | high).
 * @param trust_source_class Trust source classification (human | system |
 *        model | external).
 * @param created_at ISO-8601 UTC timestamp to use; if empty, the current
 *        UTC time is used instead.
 * @return A newly constructed ArtifactVersion with the given metadata.
 */
ArtifactVersion make_base_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& created_by_actor_type,
    const std::string& created_by_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class,
    const std::string& created_at = {});

} // namespace arcs::artifact::factory
