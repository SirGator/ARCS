add_executable(arcs_phase1_tests
    phase1_tests.cpp
)

target_link_libraries(arcs_phase1_tests
    PRIVATE
        arcs_artifact
        arcs_event
        nlohmann_json::nlohmann_json
)

add_test(NAME arcs_phase1_tests COMMAND arcs_phase1_tests)

add_executable(arcs_schema_registry_tests
    test_schema_registry.cpp
)

target_link_libraries(arcs_schema_registry_tests
    PRIVATE
        arcs_schema
)

add_test(NAME arcs_schema_registry_tests COMMAND arcs_schema_registry_tests)

find_package(GTest QUIET)

if (TARGET GTest::gtest_main)
    add_executable(arcs_store_memory_tests
        test_store_memory.cpp
    )

    target_link_libraries(arcs_store_memory_tests
        PRIVATE
            arcs_store
            GTest::gtest_main
    )

    add_test(NAME arcs_store_memory_tests COMMAND arcs_store_memory_tests)
endif()
