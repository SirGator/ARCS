#include "artifact/ids.hpp"

#include <atomic>
#include <cstdint>
#include <chrono>
#include <iomanip>
#include <random>
#include <sstream>
#include <string>

namespace arcs::artifact::ids {

namespace {
    std::atomic<std::uint64_t> g_counter{0};

    std::string make_id(const std::string& prefix) {
        const auto now = std::chrono::system_clock::now().time_since_epoch();
        const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(now).count();

        static thread_local std::mt19937_64 rng{std::random_device{}()};
        std::uniform_int_distribution<std::uint64_t> dist(0, 0xFFFFFFFFFFFF);

        const auto counter = g_counter.fetch_add(1, std::memory_order_relaxed);
        const auto rnd = dist(rng);

        std::ostringstream oss;
        oss << prefix
            << ms << "_"
            << counter << "_"
            << std::hex << rnd;

        return oss.str();
    }
} // namespace arcs::artifact::ids

std::string new_artifact_id() {
    return make_id("a_");
}

std::string new_version_id() {
    return make_id("v_");
}

std::string new_event_id() {
    return make_id("e_");
}

}
