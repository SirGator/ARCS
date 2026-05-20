add_library(arcs_interpretation
    src/interpretation_api.cpp
    src/worker_client.cpp
)

target_include_directories(arcs_interpretation
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_interpretation
    PUBLIC
        nlohmann_json::nlohmann_json
)
