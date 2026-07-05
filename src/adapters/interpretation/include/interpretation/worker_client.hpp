/**
 * @file worker_client.hpp
 * @brief Declares the concrete HTTP client that talks to the external
 *        interpretation worker.
 */

#pragma once

#include "interpretation/config.hpp"
#include "interpretation/api.hpp"

namespace arcs::interpretation {

/**
 * @brief Concrete client implementation for the external parser/worker.
 *
 * Internally builds an HTTP POST request with a JSON body and parses the
 * JSON response.
 */
class WorkerInterpretationClient final : public IInterpretationApi {
public:
    /**
     * @brief Constructs a client bound to the given worker configuration.
     * @param config Configuration describing how to reach the worker.
     */
    explicit WorkerInterpretationClient(InterpretationApiConfig config);

    /**
     * @brief Sends a single interpretation request to the worker.
     * @param request The interpretation request to send.
     * @return The structured interpretation response.
     */
    InterpretationResponse interpret(const InterpretationRequest& request) override;

private:
    InterpretationApiConfig config_;
};

} // namespace arcs::interpretation
