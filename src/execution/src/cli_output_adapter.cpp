#include "execution/cli_output_adapter.hpp"

namespace arcs::execution {

void CliTextOutputAdapter::write(std::ostream& out, const std::string& text) const
{
    out << text;
    if (!text.empty() && text.back() != '\n') {
        out << '\n';
    }
}

} // namespace arcs::execution
