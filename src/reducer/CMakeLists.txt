add_library(arcs_reducer
    src/task_state_reducer.cpp
    src/approval_state_reducer.cpp
    src/mock_time_source.cpp
    src/permission_reducer.cpp
)

target_include_directories(arcs_reducer
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_reducer
    PUBLIC
        arcs_artifact
        arcs_policy
)
