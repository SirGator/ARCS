#include "ingress/ingress_source.hpp"

#include <iostream>
#include <string>

namespace arcs::ingress {

// CLI-Eingabe: liest Zeile von stdin.
class CliIngressSource final : public IIngressSource {
public:
    explicit CliIngressSource(
        std::istream& in = std::cin,
        const std::string& source_ref = "cli",
        const std::string& actor_id = "user:cli",
        const std::string& actor_type = "human")
        : in_(in), source_ref_(source_ref), actor_id_(actor_id), actor_type_(actor_type), exhausted_(false)
    {}

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
