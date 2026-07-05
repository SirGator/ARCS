/**
 * @file api.hpp
 * @brief Defines the single-call contract for interpretation requests: a
 *        client submits one request and receives exactly one proposal back.
 */

#pragma once

#include "interpretation/config.hpp"
#include "interpretation/request.hpp"
#include "interpretation/response.hpp"

namespace arcs::interpretation {

/**
 * @brief Abstract contract for interpretation requests.
 *
 * The client sends a single request in and receives exactly one proposal
 * back, decoupling callers from the concrete interpretation backend.
 */
class IInterpretationApi {
public:
    virtual ~IInterpretationApi() = default;

    /**
     * @brief Runs the interpretation and returns the structured result.
     * @param request The interpretation request to process.
     * @return The structured interpretation response.
     */
    virtual InterpretationResponse interpret(const InterpretationRequest& request) = 0;
};

} // namespace arcs::interpretation
