add_library(arcs_execution
    ../adapters/output/src/cli_output_adapter.cpp
    src/action_materializer.cpp
    src/action_dispatcher.cpp
    src/action_handler_registry.cpp
    src/emit_report_step.cpp
    src/final_guards.cpp
    src/idempotency_store.cpp
    src/policy_update_executor.cpp
    src/report_emit_executor.cpp
    src/revocation_check.cpp
)

target_include_directories(arcs_execution
    PUBLIC
        ${PROJECT_SOURCE_DIR}/src
        ${CMAKE_CURRENT_SOURCE_DIR}/include
        ${CMAKE_CURRENT_SOURCE_DIR}/../adapters/output/include
)

target_link_libraries(arcs_execution
    PUBLIC
        arcs_artifact
        arcs_policy
        arcs_store
)
