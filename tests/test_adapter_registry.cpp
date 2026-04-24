#include <gtest/gtest.h>

#include <memory>

#include "execution/adapter_executor.hpp"
#include "execution/adapter_registry.hpp"
#include "execution/execution_result.hpp"

namespace {

class DummyAdapter final : public arcs::execution::IExecutionAdapter {
public:
    std::string handles_action_type() const override
    {
        return "dummy";
    }

    arcs::execution::ExecutionResult execute(
        const arcs::execution::Action& action,
        const arcs::execution::ExecutionContext&) override
    {
        return arcs::execution::ExecutionResult::success({action.artifact_id, action.version_id});
    }
};

TEST(AdapterRegistryTest, RoutesToRegisteredAdapter)
{
    arcs::execution::AdapterRegistry registry;
    ASSERT_TRUE(registry.register_adapter(std::make_shared<DummyAdapter>()));

    arcs::execution::AdapterExecutor executor(registry);

    arcs::execution::Action action{};
    action.artifact_id = "a_1";
    action.version_id = "v_1";
    action.payload.type = "dummy";

    arcs::execution::ExecutionContext ctx{};
    const auto result = executor.execute(action, ctx);

    EXPECT_EQ(result.status, arcs::execution::ExecutionStatus::Success);
    EXPECT_EQ(result.action_ref.artifact_id, "a_1");
    EXPECT_EQ(result.action_ref.version_id, "v_1");
}

TEST(AdapterRegistryTest, UnknownTypeFails)
{
    arcs::execution::AdapterRegistry registry;
    arcs::execution::AdapterExecutor executor(registry);

    arcs::execution::Action action{};
    action.artifact_id = "a_1";
    action.version_id = "v_1";
    action.payload.type = "missing";

    arcs::execution::ExecutionContext ctx{};
    const auto result = executor.execute(action, ctx);

    EXPECT_EQ(result.status, arcs::execution::ExecutionStatus::Fail);
    EXPECT_FALSE(result.error_message.empty());
}

} // namespace
