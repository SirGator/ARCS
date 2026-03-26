add_library(arcs_event
    src/json.cpp
)

target_include_directories(arcs_event
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_event
    PUBLIC
        arcs_artifact
        nlohmann_json::nlohmann_json
)
