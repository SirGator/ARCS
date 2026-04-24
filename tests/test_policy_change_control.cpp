#include <cassert>
#include <iostream>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "verification/authority_verifier.hpp"

using namespace arcs;

static artifact::ArtifactVersion make_policy_update_action()
{
    artifact::ArtifactVersion action{};
    action.artifact_id = "a_policy_update_action";
    action.version_id = "v_policy_update_action_1";
    action.version = 1;
    action.type = "action";
    action.schema_id = "arcs.action.v1";
    action.schema_version = 1;
    action.stream_key = "policy:core";

    action.payload = nlohmann::json{
        {"type", "policy_update"},
        {"params", {
            {"policy_artifact_id", "a_policy_core"},
            {"policy_version_id", "v_policy_002"},
            {"policy_version", 2},
            {"event_id", "e_policy_002"},
            {"new_policy", {
                {"capabilities", {"exec:report_emit", "policy:edit"}},
                {"constraints", nlohmann::json::object()},
                {"verifier_rules", {
                    {"hard_checks", {"schema", "permission", "authority"}},
                    {"soft_checks", nlohmann::json::array()}
                }},
                {"approval_required_for", {"policy:edit"}}
            }}
        }}
    };

    return action;
}

static verification::VerificationContext make_ctx_without_authority()
{
    verification::VerificationContext ctx{};
    ctx.permissions.principal = "user:simon";
    ctx.permissions.capabilities = {
        "exec:report_emit"
    };
    return ctx;
}

static verification::VerificationContext make_ctx_with_authority()
{
    verification::VerificationContext ctx{};
    ctx.permissions.principal = "user:simon";
    ctx.permissions.capabilities = {
        "exec:report_emit",
        "policy:edit"
    };
    return ctx;
}

static void policy_change_without_policy_edit_is_blocked()
{
    verification::AuthorityVerifier verifier;

    auto action = make_policy_update_action();
    auto ctx = make_ctx_without_authority();

    auto report = verifier.check(action, ctx);

    assert(report.status == "fail");
    assert(!report.blockers.empty());

    std::cout << "[PASS] policy change without policy:edit is blocked\n";
}

static void policy_change_with_policy_edit_is_allowed()
{
    verification::AuthorityVerifier verifier;

    auto action = make_policy_update_action();
    auto ctx = make_ctx_with_authority();

    auto report = verifier.check(action, ctx);

    assert(report.status == "pass");
    assert(report.blockers.empty());

    std::cout << "[PASS] policy change with policy:edit is allowed\n";
}

int main()
{
    policy_change_without_policy_edit_is_blocked();
    policy_change_with_policy_edit_is_allowed();

    std::cout << "test_policy_change_control OK\n";
    return 0;
}