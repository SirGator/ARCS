add_executable(arcs_phase1_tests
    phase1_tests.cpp
)

target_link_libraries(arcs_phase1_tests
    PRIVATE
        arcs_artifact
        arcs_event
        nlohmann_json::nlohmann_json
)

add_test(NAME arcs_phase1_tests COMMAND $<TARGET_FILE:arcs_phase1_tests>)

add_executable(arcs_schema_registry_tests
    test_schema_registry.cpp
)

target_link_libraries(arcs_schema_registry_tests
    PRIVATE
        arcs_schema
)

add_test(NAME arcs_schema_registry_tests COMMAND $<TARGET_FILE:arcs_schema_registry_tests>)

add_executable(arcs_store_memory_tests
    test_store_memory.cpp
)

target_link_libraries(arcs_store_memory_tests
    PRIVATE
        arcs_store
        GTest::gtest_main
)

add_test(NAME arcs_store_memory_tests COMMAND $<TARGET_FILE:arcs_store_memory_tests>)

add_executable(arcs_approval_tests
    test_approval.cpp
)

target_link_libraries(arcs_approval_tests
    PRIVATE
        arcs_approval
        GTest::gtest_main
)

add_test(NAME arcs_approval_tests COMMAND $<TARGET_FILE:arcs_approval_tests>)

add_executable(arcs_materializer_tests
    test_materializer.cpp
)

target_link_libraries(arcs_materializer_tests
    PRIVATE
        arcs_execution
        GTest::gtest_main
)

add_test(NAME arcs_materializer_tests COMMAND $<TARGET_FILE:arcs_materializer_tests>)

add_executable(arcs_policy_drift_tests
    test_policy_drift.cpp
)

target_link_libraries(arcs_policy_drift_tests
    PRIVATE
        arcs_approval
        arcs_artifact
        GTest::gtest_main
)

add_test(NAME arcs_policy_drift_tests COMMAND $<TARGET_FILE:arcs_policy_drift_tests>)

add_executable(arcs_task_state_reducer_tests
    test_task_state_reducer.cpp
)

target_link_libraries(arcs_task_state_reducer_tests
    PRIVATE
        arcs_reducer
        GTest::gtest_main
)

add_test(NAME arcs_task_state_reducer_tests COMMAND $<TARGET_FILE:arcs_task_state_reducer_tests>)

add_executable(arcs_approval_state_reducer_tests
    test_approval_state_reducer.cpp
)

target_link_libraries(arcs_approval_state_reducer_tests
    PRIVATE
        arcs_reducer
        GTest::gtest_main
)

add_test(NAME arcs_approval_state_reducer_tests COMMAND $<TARGET_FILE:arcs_approval_state_reducer_tests>)

add_executable(arcs_replay_tests
    test_replay.cpp
)

target_link_libraries(arcs_replay_tests
    PRIVATE
        arcs_reducer
        GTest::gtest_main
)

add_test(NAME arcs_replay_tests COMMAND $<TARGET_FILE:arcs_replay_tests>)

add_executable(arcs_verification_engine_tests
    test_verification_engine.cpp
)

target_link_libraries(arcs_verification_engine_tests
    PRIVATE
        arcs_verification
        GTest::gtest_main
)

add_test(NAME arcs_verification_engine_tests COMMAND $<TARGET_FILE:arcs_verification_engine_tests>)

add_executable(arcs_unknown_blocks_tests
    test_unknown_blocks.cpp
)

target_link_libraries(arcs_unknown_blocks_tests
    PRIVATE
        arcs_verification
        GTest::gtest_main
)

add_test(NAME arcs_unknown_blocks_tests COMMAND $<TARGET_FILE:arcs_unknown_blocks_tests>)

add_executable(arcs_happy_path_tests
    test_happy_path.cpp
)

target_link_libraries(arcs_happy_path_tests
    PRIVATE
        arcs_execution
)

add_test(NAME arcs_happy_path_tests COMMAND $<TARGET_FILE:arcs_happy_path_tests>)

add_executable(arcs_idempotency_tests
    test_idempotency.cpp
)

target_link_libraries(arcs_idempotency_tests
    PRIVATE
        arcs_execution
)

add_test(NAME arcs_idempotency_tests COMMAND $<TARGET_FILE:arcs_idempotency_tests>)

add_executable(arcs_revocation_tests
    test_revocation.cpp
)

target_link_libraries(arcs_revocation_tests
    PRIVATE
        arcs_execution
)

add_test(NAME arcs_revocation_tests COMMAND $<TARGET_FILE:arcs_revocation_tests>)

add_executable(arcs_llm_isolation_tests
    test_llm_isolation.cpp
)

add_test(NAME arcs_llm_isolation_tests COMMAND $<TARGET_FILE:arcs_llm_isolation_tests>)
