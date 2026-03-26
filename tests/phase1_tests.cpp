#include <stdexcept>
#include <string>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "artifact/json.hpp"
#include "event/event.hpp"
#include "event/json.hpp"

namespace {

void require(bool condition, const std::string& message)
{
    if (!condition)
    {
        throw std::runtime_error(message);
    }
}

void test_artifact_roundtrip()
{
    arcs::ArtifactVersion artifact;
    artifact.artifact_id = "a_test";
    artifact.version_id = "v_test";
    artifact.version = 2;
    artifact.type = "task";
    artifact.schema_id = "arcs.task.v1";
    artifact.schema_version = 1;
    artifact.created_at = "2026-03-22T12:30:00Z";
    artifact.created_by = {"human", "user:simon"};
    artifact.source = {"chat", "local-cli"};
    artifact.trust = {"high", "human"};
    artifact.stream_key = "task_id:t_1";
    artifact.tags = {"phase1", "roundtrip"};
    artifact.payload = {{"title", "Artifact Test"}, {"priority", "high"}};
    artifact.provenance.parents = {"a_parent_1", "a_parent_2"};
    artifact.provenance.rules_applied = {"rule_a"};
    artifact.provenance.models_used = {{"gpt", "sha256:abc", {"a_parent_1"}, 0.2, "sha256:def"}};
    artifact.provenance.transform = "reason";

    const nlohmann::json encoded = artifact;
    const arcs::ArtifactVersion decoded = encoded.get<arcs::ArtifactVersion>();

    require(decoded.artifact_id == artifact.artifact_id, "artifact_id mismatch");
    require(decoded.created_by.actor_type == "human", "created_by mismatch");
    require(decoded.source.kind == "chat", "source mismatch");
    require(decoded.tags.size() == 2, "tags mismatch");
    require(decoded.provenance.parents.size() == 2, "provenance parents mismatch");
    require(decoded.provenance.models_used.size() == 1, "models_used mismatch");
}

void test_event_roundtrip()
{
    arcs::Event event;
    event.event_id = "e_test";
    event.event_type = "artifact_committed";
    event.ts = "2026-03-22T12:30:01Z";
    event.actor = {"system", "phase1-tests"};
    event.refs = {{"a_test", "v_test", "target"}};
    event.stream_key = "task_id:t_1";
    event.payload = {{"status", "ok"}};
    event.prev_hash = "hash_123";

    const nlohmann::json encoded = event;
    const arcs::Event decoded = encoded.get<arcs::Event>();

    require(decoded.event_id == event.event_id, "event_id mismatch");
    require(decoded.actor.id == event.actor.id, "event actor mismatch");
    require(decoded.refs.size() == 1, "event refs mismatch");
    require(decoded.refs.front().role == "target", "event ref role mismatch");
    require(decoded.prev_hash == "hash_123", "prev_hash mismatch");
}

void test_invalid_actor_type_rejected()
{
    const nlohmann::json invalid = {
        {"artifact_id", "a_bad"},
        {"version_id", "v_bad"},
        {"version", 1},
        {"type", "task"},
        {"schema_id", "arcs.task.v1"},
        {"schema_version", 1},
        {"created_at", "2026-03-22T12:30:02Z"},
        {"created_by", {{"actor_type", "robot"}, {"id", "bad"}}},
        {"source", {{"kind", "chat"}, {"ref", "phase1-tests"}}},
        {"trust", {{"level", "high"}, {"source_class", "human"}}},
        {"stream_key", "task_id:t_1"},
        {"tags", nlohmann::json::array()},
        {"payload", {{"title", "Bad"}}},
        {"provenance", {{"parents", nlohmann::json::array()}, {"rules_applied", nlohmann::json::array()}, {"models_used", nlohmann::json::array()}, {"transform", "extract"}}}
    };

    bool threw = false;
    try
    {
        static_cast<void>(invalid.get<arcs::ArtifactVersion>());
    }
    catch (const std::invalid_argument&)
    {
        threw = true;
    }

    require(threw, "invalid actor_type should throw");
}

void test_missing_required_field_rejected()
{
    const nlohmann::json invalid = {
        {"version_id", "v_bad"},
        {"version", 1},
        {"type", "task"},
        {"schema_id", "arcs.task.v1"},
        {"schema_version", 1},
        {"created_at", "2026-03-22T12:30:03Z"},
        {"created_by", {{"actor_type", "human"}, {"id", "user:simon"}}},
        {"source", {{"kind", "chat"}, {"ref", "phase1-tests"}}},
        {"trust", {{"level", "high"}, {"source_class", "human"}}},
        {"stream_key", "task_id:t_1"},
        {"tags", nlohmann::json::array()},
        {"payload", {{"title", "Bad"}}},
        {"provenance", {{"parents", nlohmann::json::array()}, {"rules_applied", nlohmann::json::array()}, {"models_used", nlohmann::json::array()}, {"transform", "extract"}}}
    };

    bool threw = false;
    try
    {
        static_cast<void>(invalid.get<arcs::ArtifactVersion>());
    }
    catch (const nlohmann::json::out_of_range&)
    {
        threw = true;
    }

    require(threw, "missing artifact_id should throw");
}

void test_provenance_roundtrip_explicit()
{
    arcs::Provenance provenance;
    provenance.parents = {"a_1", "a_2"};
    provenance.rules_applied = {"rule_permission_check", "rule_scope"};
    provenance.models_used = {
        {"gpt-5", "sha256:prompt", {"a_1", "a_2"}, 0.1, "sha256:output"}
    };
    provenance.transform = "verify";

    const nlohmann::json encoded = provenance;
    const arcs::Provenance decoded = encoded.get<arcs::Provenance>();

    require(decoded.parents == provenance.parents, "provenance parents mismatch");
    require(decoded.rules_applied == provenance.rules_applied, "provenance rules mismatch");
    require(decoded.models_used.size() == 1, "provenance models size mismatch");
    require(decoded.models_used.front().name == "gpt-5", "provenance model name mismatch");
    require(decoded.models_used.front().inputs.size() == 2, "provenance model inputs mismatch");
    require(decoded.transform == "verify", "provenance transform mismatch");
}

void test_invalid_source_kind_rejected()
{
    const nlohmann::json invalid = {
        {"artifact_id", "a_bad"},
        {"version_id", "v_bad"},
        {"version", 1},
        {"type", "task"},
        {"schema_id", "arcs.task.v1"},
        {"schema_version", 1},
        {"created_at", "2026-03-22T12:30:04Z"},
        {"created_by", {{"actor_type", "human"}, {"id", "user:simon"}}},
        {"source", {{"kind", "unknown"}, {"ref", "phase1-tests"}}},
        {"trust", {{"level", "high"}, {"source_class", "human"}}},
        {"stream_key", "task_id:t_1"},
        {"tags", nlohmann::json::array()},
        {"payload", {{"title", "Bad"}}},
        {"provenance", {{"parents", nlohmann::json::array()}, {"rules_applied", nlohmann::json::array()}, {"models_used", nlohmann::json::array()}, {"transform", "extract"}}}
    };

    bool threw = false;
    try
    {
        static_cast<void>(invalid.get<arcs::ArtifactVersion>());
    }
    catch (const std::invalid_argument&)
    {
        threw = true;
    }

    require(threw, "invalid source.kind should throw");
}

} // namespace

int main()
{
    test_artifact_roundtrip();
    test_event_roundtrip();
    test_invalid_actor_type_rejected();
    test_missing_required_field_rejected();
    test_provenance_roundtrip_explicit();
    test_invalid_source_kind_rejected();
    return 0;
}
