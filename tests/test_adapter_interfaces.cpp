#include <gtest/gtest.h>

#include <type_traits>

#include "adapters/common.hpp"
#include "adapters/database/database_adapter.hpp"
#include "adapters/external_state/external_state_adapter.hpp"
#include "adapters/input/input_adapter.hpp"
#include "adapters/interpretation/interpretation_adapter.hpp"
#include "adapters/llm/llm_adapter.hpp"
#include "adapters/output/output_adapter.hpp"
#include "adapters/reasoning/reasoning_adapter.hpp"

TEST(AdapterInterfacesTest, CommonStructuresHaveExpectedDefaults)
{
    const arcs::adapters::LocalValidationResult validation{};
    EXPECT_FALSE(validation.ok);
    EXPECT_TRUE(validation.diagnostics.is_object());

    const arcs::adapters::CoreSubmissionResult submission{};
    EXPECT_FALSE(submission.accepted);

    const arcs::adapters::AdapterInfo info{};
    EXPECT_TRUE(info.id.empty());
    EXPECT_TRUE(info.capabilities.empty());

    const arcs::adapters::AdapterHealth health{};
    EXPECT_TRUE(health.ok);
    EXPECT_EQ(health.status, "ok");
}

TEST(AdapterInterfacesTest, AllAdapterInterfacesDeriveFromBaseAdapter)
{
    EXPECT_TRUE((std::is_base_of_v<arcs::adapters::IAdapter, arcs::adapters::input::IInputAdapter>));
    EXPECT_TRUE((std::is_base_of_v<arcs::adapters::IAdapter, arcs::adapters::external_state::IExternalStateAdapter>));
    EXPECT_TRUE((std::is_base_of_v<arcs::adapters::IAdapter, arcs::adapters::output::IOutputAdapter>));
    EXPECT_TRUE((std::is_base_of_v<arcs::adapters::IAdapter, arcs::adapters::database::IDatabaseAdapter>));
    EXPECT_TRUE((std::is_base_of_v<arcs::adapters::IAdapter, arcs::adapters::interpretation::IInterpretationAdapter>));
    EXPECT_TRUE((std::is_base_of_v<arcs::adapters::IAdapter, arcs::adapters::reasoning::IReasoningAdapter>));
    EXPECT_TRUE((std::is_base_of_v<arcs::adapters::IAdapter, arcs::adapters::llm::ILlmAdapter>));
}
