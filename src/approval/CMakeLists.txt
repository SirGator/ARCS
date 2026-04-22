add_library(arcs_approval
    src/approval_gate.cpp
)

target_include_directories(arcs_approval
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_approval
    PUBLIC
        arcs_artifact
)
