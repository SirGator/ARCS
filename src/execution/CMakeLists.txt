add_library(arcs_execution
    src/action_materializer.cpp
    src/emit_report_step.cpp
    src/final_guards.cpp
    src/idempotency_store.cpp
    src/report_emit_executor.cpp
    src/revocation_check.cpp
)

target_include_directories(arcs_execution
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_execution
    PUBLIC
        arcs_artifact
        arcs_policy
)
