add_library(arcs_interpretation
    src/api_interpreter.cpp
    src/interpretation_to_task_mapper.cpp
    src/rule_based_interpreter.cpp
)

target_include_directories(arcs_interpretation
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_interpretation
    PUBLIC
        nlohmann_json::nlohmann_json
)

target_compile_features(arcs_interpretation PRIVATE cxx_std_20)
