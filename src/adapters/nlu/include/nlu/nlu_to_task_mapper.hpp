#pragma once

#include "nlu_result.hpp"
#include "artifact/artifact.hpp"

namespace arcs::nlu {

class NluToTaskMapper {
public:
    arcs::artifact::ArtifactVersion map_to_task(const NluResult& nlu);
};

}
