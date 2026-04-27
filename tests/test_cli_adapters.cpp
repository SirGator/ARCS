#include <gtest/gtest.h>

#include <sstream>
#include <string>

#include "core/flow.hpp"
#include "input/input_adapter.hpp"
#include "execution/cli_output_adapter.hpp"

TEST(CliAdaptersTest, ReadInputAndWriteOutput)
{
    std::istringstream in("approval=yes permission=no\n");
    std::ostringstream out;

    arcs::input::CliTextInputAdapter input_adapter;
    arcs::execution::CliTextOutputAdapter output_adapter;

    const auto input_artifact = input_adapter.read_artifact(in, "cli", "session:test");
    const auto result = arcs::core::run_text_flow(input_artifact);

    output_adapter.write(out, result);

    const auto output = out.str();
    EXPECT_NE(output.find("step: ingress_event -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: task -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: option -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: check permission -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("permission: capability exec:report_emit fehlt"), std::string::npos);
}
