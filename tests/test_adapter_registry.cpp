#include <gtest/gtest.h>

#include <memory>

#include "execution/action_dispatcher.hpp"
#include "execution/action_handler_registry.hpp"
#include "execution/executor.hpp"
#include "execution/execution_result.hpp"

namespace {

class DummyHandler final : public arcs::execution::IExecutor {
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

TEST(ActionHandlerRegistryTest, RoutesToRegisteredHandler)
{
    arcs::execution::ActionHandlerRegistry registry;
    ASSERT_TRUE(registry.register_handler(std::make_shared<DummyHandler>()));

    arcs::execution::ActionDispatcher dispatcher(registry);

    arcs::execution::Action action{};
    action.artifact_id = "a_1";
    action.version_id = "v_1";
    action.payload.type = "dummy";

    arcs::execution::ExecutionContext ctx{};
    const auto result = dispatcher.execute(action, ctx);

    EXPECT_EQ(result.status, arcs::execution::ExecutionStatus::Success);
    EXPECT_EQ(result.action_ref.artifact_id, "a_1");
    EXPECT_EQ(result.action_ref.version_id, "v_1");
}

TEST(ActionHandlerRegistryTest, UnknownTypeFails)
{
    arcs::execution::ActionHandlerRegistry registry;
    arcs::execution::ActionDispatcher dispatcher(registry);

    arcs::execution::Action action{};
    action.artifact_id = "a_1";
    action.version_id = "v_1";
    action.payload.type = "missing";

    arcs::execution::ExecutionContext ctx{};
    const auto result = dispatcher.execute(action, ctx);

    EXPECT_EQ(result.status, arcs::execution::ExecutionStatus::Fail);
    EXPECT_FALSE(result.error_message.empty());
}

} // namespace
