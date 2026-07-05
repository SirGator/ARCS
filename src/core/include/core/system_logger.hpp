/**
 * @file system_logger.hpp
 * @brief Lightweight step logger that records per-stage ok/fail outcomes for
 *        a flow run and renders them as a text log.
 */

#pragma once

#include <sstream>
#include <string>
#include <vector>

namespace arcs::core {

/** @brief Outcome of a single logged step. */
enum class StepStatus {
    Ok,
    Fail
};

/** @brief A single recorded step: its name, outcome, and optional detail. */
struct StepLogEntry {
    std::string name;
    StepStatus status{StepStatus::Ok};
    std::string detail;
};

/**
 * @brief Accumulates a sequential log of named step outcomes for a flow run
 *        and can report whether all steps succeeded or render the log as
 *        text.
 */
class SystemLogger {
public:
    /**
     * @brief Records a successful step.
     * @param name Name of the step.
     * @param detail Optional extra detail to record.
     */
    void ok(const std::string& name, const std::string& detail = {})
    {
        entries_.push_back(StepLogEntry{name, StepStatus::Ok, detail});
    }

    /**
     * @brief Records a failed step.
     * @param name Name of the step.
     * @param detail Detail explaining the failure.
     */
    void fail(const std::string& name, const std::string& detail)
    {
        entries_.push_back(StepLogEntry{name, StepStatus::Fail, detail});
    }

    /** @brief Returns true if no step has been recorded as failed. */
    bool all_ok() const
    {
        for (const auto& entry : entries_) {
            if (entry.status == StepStatus::Fail) {
                return false;
            }
        }
        return true;
    }

    /** @brief Renders all recorded steps as a multi-line text log. */
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

    const std::vector<StepLogEntry>& entries() const
    {
        return entries_;
    }

private:
    std::vector<StepLogEntry> entries_;
};

} // namespace arcs::core
