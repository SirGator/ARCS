add_executable(arcs_app
    main.cpp
)

target_link_libraries(arcs_app
    PRIVATE
        arcs_core
        arcs_input_adapter
        arcs_execution
)
