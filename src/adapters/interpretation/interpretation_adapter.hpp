#pragma once

#include "adapters/common.hpp"
#include "interpretation/request.hpp"
#include "interpretation/response.hpp"

namespace arcs::adapters::interpretation {

class IInterpretationAdapter : public arcs::adapters::IAdapter {
public:
    ~IInterpretationAdapter() override = default;

    virtual arcs::interpretation::InterpretationResponse interpret(
        const arcs::interpretation::InterpretationRequest& request) = 0;
};

} // namespace arcs::adapters::interpretation
