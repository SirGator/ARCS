#pragma once

#include <ostream>
#include <string>

namespace arcs::execution {

class CliTextOutputAdapter {
public:
    void write(std::ostream& out, const std::string& text) const;
};

} // namespace arcs::execution
