/**
 * @file cli_ingress_source.hpp
 * @brief Defines CliIngressSource, an IIngressSource implementation that
 *        reads ingress events line-by-line from an input stream (stdin by
 *        default), for interactive/CLI-driven use of ARCS.
 */
#include "ingress/ingress_source.hpp"

#include <iostream>
#include <string>

namespace arcs::ingress {

/**
 * @brief IIngressSource implementation that reads one line at a time from
 *        an input stream (e.g. stdin) and wraps each line as a "chat"
 *        IngressEvent. Becomes exhausted once the stream reports EOF/failure.
 */
// CLI-Eingabe: liest Zeile von stdin.
class CliIngressSource final : public IIngressSource {
public:
    /**
     * @brief Constructs a CLI ingress source bound to the given input
     *        stream and default event metadata.
     * @param in Input stream to read lines from (defaults to std::cin).
     * @param source_ref Value to record as the event's source reference.
     * @param actor_id Identifier of the actor to attribute emitted events to.
     * @param actor_type Type of the actor to attribute emitted events to.
     */
    explicit CliIngressSource(
        std::istream& in = std::cin,
        const std::string& source_ref = "cli",
        const std::string& actor_id = "user:cli",
        const std::string& actor_type = "human")
        : in_(in), source_ref_(source_ref), actor_id_(actor_id), actor_type_(actor_type), exhausted_(false)
    {}

    /**
     * @brief Reads the next line from the input stream and wraps it as an
     *        IngressEvent. Marks the source as exhausted if no more input
     *        is available.
     * @return The IngressEvent for the read line, or a default-constructed
     *         IngressEvent if the stream is exhausted.
     */
    IngressEvent emit() override
    {
        std::string line;
        if (!std::getline(in_, line)) {
            exhausted_ = true;
            return IngressEvent{};
        }

        IngressEvent event;
        event.source_kind = "chat";
        event.source_ref = source_ref_;
        event.raw_payload = line;
        event.actor_id = actor_id_;
        event.actor_type = actor_type_;
        return event;
    }

    /**
     * @brief Reports whether the underlying stream may still yield more
     *        input.
     * @return True if the stream has not yet been exhausted.
     */
    bool has_more() const override
    {
        return !exhausted_;
    }

private:
    std::istream& in_;
    std::string source_ref_;
    std::string actor_id_;
    std::string actor_type_;
    bool exhausted_;
};

} // namespace arcs::ingress
