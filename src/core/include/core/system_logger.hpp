#pragma once

#include <sstream>
#include <string>
#include <vector>

namespace arcs::core {

enum class StepStatus {
    Ok,
    Fail
};

struct StepLogEntry {
    std::string name;
    StepStatus status{StepStatus::Ok};
    std::string detail;
};

class SystemLogger {
public:
    void ok(const std::string& name, const std::string& detail = {})
    {
        entries_.push_back(StepLogEntry{name, StepStatus::Ok, detail});
    }

    void fail(const std::string& name, const std::string& detail)
    {
        entries_.push_back(StepLogEntry{name, StepStatus::Fail, detail});
    }

    bool all_ok() const
    {
        for (const auto& entry : entries_) {
            if (entry.status == StepStatus::Fail) {
                return false;
            }
        }
        return true;
    }

    std::string format() const
    {
        std::ostringstream out;
        for (const auto& entry : entries_) {
            out << "step: " << entry.name << " -> "
                << (entry.status == StepStatus::Ok ? "OK" : "FAIL");
            if (!entry.detail.empty()) {
                out << " | " << entry.detail;
            }
            out << '\n';
        }
        return out.str();
    }

private:
    std::vector<StepLogEntry> entries_;
};

} // namespace arcs::core
