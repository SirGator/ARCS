add_library(arcs_core
    src/flow.cpp
)

target_include_directories(arcs_core
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_core
    PUBLIC
        arcs_approval
        arcs_artifact
        arcs_event
        arcs_execution
        arcs_nlu
        arcs_store
        arcs_verification
)
