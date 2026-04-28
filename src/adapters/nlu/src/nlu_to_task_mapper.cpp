#include "nlu/nlu_to_task_mapper.hpp"

#include <algorithm>
#include <cctype>

#include "artifact/factory.hpp"

namespace arcs::nlu {

namespace {

std::string humanize_intent(std::string intent)
{
    std::replace(intent.begin(), intent.end(), '_', ' ');
    if (!intent.empty()) {
        intent.front() = static_cast<char>(std::toupper(static_cast<unsigned char>(intent.front())));
    }
    return intent;
}

bool is_valid_priority(const std::string& value)
{
    return value == "low" || value == "medium" || value == "high";
}

} // namespace

arcs::artifact::ArtifactVersion NluToTaskMapper::map_to_task(const NluResult& nlu)
{
    auto task = arcs::artifact::factory::make_base_artifact(
        "task",
        "arcs.task.v1",
        "nlu:task",
        "system",
        "nlu-mapper",
        "internal",
        "nlu",
        "low",
        "model");

    const auto title = !nlu.intent.empty() ? humanize_intent(nlu.intent) : std::string{"unknown"};
    task.payload = {
        {"title", title},
        {"description", nlu.raw_text},
    };

    const auto topic_it = nlu.entities.find("topic");
    if (topic_it != nlu.entities.end() && !topic_it->second.empty()) {
        task.payload["scope"] = topic_it->second;
    }

    const auto priority_it = nlu.entities.find("priority");
    if (priority_it != nlu.entities.end() && is_valid_priority(priority_it->second)) {
        task.payload["priority"] = priority_it->second;
    }

    task.provenance.rules_applied = {"nlu_to_task_mapper"};
    task.provenance.transform = "nlu_to_task";
    return task;
}

} // namespace arcs::nlu
