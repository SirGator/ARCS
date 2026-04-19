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
