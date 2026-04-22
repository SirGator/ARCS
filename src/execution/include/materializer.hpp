#pragma once

#include <vector>

namespace arcs::artifact {
struct ArtifactVersion;
}

namespace arcs::execution {

using OptionArtifact = arcs::artifact::ArtifactVersion;
using PolicyArtifact = arcs::artifact::ArtifactVersion;
using ActionArtifact = arcs::artifact::ArtifactVersion;

class IMaterializer {
public:
    virtual ~IMaterializer() = default;

    virtual std::vector<ActionArtifact>
    materialize(const OptionArtifact& option,
                const PolicyArtifact& policy) const = 0;
};

class ActionMaterializer final : public IMaterializer {
public:
    std::vector<ActionArtifact>
    materialize(const OptionArtifact& option,
                const PolicyArtifact& policy) const override;
};

} // namespace arcs::execution
