#include <gtest/gtest.h>

#include "core/services/ingress_service.hpp"

TEST(IngressServiceTest, AcceptsExternalSignalDraftWithoutCliSource)
{
    arcs::core::services::IngressService service;
    arcs::ingress::QuarantineStore quarantine;

    const auto result = service.run(
        arcs::core::services::IngressDraft{
            .signal = arcs::core::services::ExternalSignal{
                .source_kind = "http",
                .source_ref = "/api/messages",
                .raw_payload = "hello world",
                .stream_key = "session:http",
                .actor_id = "user:http",
                .actor_type = "human",
            }},
        quarantine);

    EXPECT_TRUE(result.success);
    EXPECT_EQ(result.ingress_artifact.type, "ingress_event");
    EXPECT_EQ(result.ingress_artifact.stream_key, "session:http");
    EXPECT_EQ(result.ingress_artifact.payload.value("source_kind", std::string{}), "http");
    EXPECT_EQ(result.ingress_artifact.payload.value("raw_text", std::string{}), "hello world");
}
