add_library(arcs_nlu
    src/rule_based_nlu_parser.cpp
    src/model_nlu_parser.cpp
    src/nlu_to_task_mapper.cpp
)

target_include_directories(arcs_nlu
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_nlu
    PUBLIC
        arcs_artifact
)
