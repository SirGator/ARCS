add_library(arcs_artifact
    src/ids.cpp
    src/json.cpp
)

target_include_directories(arcs_artifact
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_artifact
    PUBLIC
        nlohmann_json::nlohmann_json
)
