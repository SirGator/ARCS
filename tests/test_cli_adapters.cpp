#include <gtest/gtest.h>

#include <sstream>
#include <string>

#include "core/flow.hpp"
#include "execution/cli_output_adapter.hpp"
#include "ingress/ingress_normalizer.hpp"
#include "ingress/cli_ingress_source.hpp"

TEST(CliAdaptersTest, ReadInputAndWriteOutput)
{
    std::istringstream in("approval=yes permission=no\n");
    std::ostringstream out;

    arcs::execution::CliTextOutputAdapter output_adapter;

    arcs::ingress::CliIngressSource source(in, "cli", "user:cli", "human");
    auto raw_event = source.emit();
    arcs::ingress::DefaultIngressNormalizer normalizer("session:test");
    auto normalized = normalizer.normalize(raw_event);

    const auto result = arcs::core::run_text_flow(normalized.artifact);

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
