#pragma once
#include <string>

namespace arcs::reducer {

using Timestamp = std::string;

class ITimeSource {
public:
    virtual ~ITimeSource() = default;
    virtual Timestamp now() const = 0;
};

} // namespace arcs::reducer
