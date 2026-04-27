add_library(arcs_input_adapter
    src/input_adapter.cpp
)

target_include_directories(arcs_input_adapter
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_input_adapter
    PUBLIC
        arcs_artifact
)
