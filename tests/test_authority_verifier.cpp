#include <cassert>
#include <iostream>

#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "verification/authority_verifier.hpp"

using namespace arcs;

static artifact::ArtifactVersion make_policy_update_action()
{
    artifact::ArtifactVersion action{};
    action.artifact_id = "a_action_policy_update";
    action.version_id = "v_action_policy_update_1";
    action.type = "action";
    action.schema_id = "arcs.action.v1";
    action.schema_version = 1;

    action.payload = nlohmann::json{
        {"type", "policy_update"},
        {"params", nlohmann::json::object()}
    };

    return action;
}

static verification::VerificationContext make_ctx_without_policy_edit()
{
    verification::VerificationContext ctx{};
    ctx.permissions.principal = "user:simon";
    ctx.permissions.capabilities = {
        "exec:report_emit"
    };
    return ctx;
}

static verification::VerificationContext make_ctx_with_policy_edit()
{
    verification::VerificationContext ctx{};
    ctx.permissions.principal = "user:simon";
    ctx.permissions.capabilities = {
        "exec:report_emit",
        "policy:edit"
    };
    return ctx;
}

static void policy_update_without_capability_fails()
{
    verification::AuthorityVerifier verifier;

    auto action = make_policy_update_action();
    auto ctx = make_ctx_without_policy_edit();

    auto report = verifier.check(action, ctx);

    assert(report.status == "fail");
    assert(!report.blockers.empty());

    std::cout << "[PASS] policy_update without policy:edit fails\n";
}

static void policy_update_with_capability_passes()
{
    verification::AuthorityVerifier verifier;

    auto action = make_policy_update_action();
    auto ctx = make_ctx_with_policy_edit();

    auto report = verifier.check(action, ctx);

    assert(report.status == "pass");
    assert(report.blockers.empty());

    std::cout << "[PASS] policy_update with policy:edit passes\n";
}

int main()
{
    policy_update_without_capability_fails();
    policy_update_with_capability_passes();

    std::cout << "test_authority_verifier OK\n";
    return 0;
}