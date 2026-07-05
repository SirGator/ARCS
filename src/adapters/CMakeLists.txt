add_library(arcs_adapters INTERFACE)

target_include_directories(arcs_adapters
    INTERFACE
        ${PROJECT_SOURCE_DIR}/src
        ${PROJECT_SOURCE_DIR}/src/core/include
        ${PROJECT_SOURCE_DIR}/src/adapters/interpretation/include
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_adapters
    INTERFACE
        arcs_artifact
        nlohmann_json::nlohmann_json
)

add_subdirectory(interpretation)
