/**
 * @file cli_output_adapter.cpp
 * @brief Implements CliTextOutputAdapter::write.
 */

#include "adapters/output/cli_output_adapter.hpp"

namespace arcs::adapters::output {

void CliTextOutputAdapter::write(std::ostream& out, const std::string& text) const
{
    out << text;
    if (!text.empty() && text.back() != '\n') {
        out << '\n';
    }
}

} // namespace arcs::adapters::output
