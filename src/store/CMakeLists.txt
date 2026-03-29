add_library(arcs_store
    src/optimistic_lock.cpp
    src/head_tracker.cpp
    src/store_memory.cpp
)

target_include_directories(arcs_store
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_store
    PUBLIC
        arcs_artifact
        arcs_event
)
