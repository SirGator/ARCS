add_executable(arcs_app
    main.cpp
)

target_link_libraries(arcs_app
    PRIVATE
        arcs_artifact
        arcs_event
        arcs_approval
        arcs_execution
        arcs_reducer
        arcs_store
        arcs_schema
        arcs_verification
        nlohmann_json::nlohmann_json
)

target_compile_definitions(arcs_app
    PRIVATE
        ARCS_SOURCE_DIR="${CMAKE_SOURCE_DIR}"
)
