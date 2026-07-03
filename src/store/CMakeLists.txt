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
        arcs_schema
)

# Optional persistent store (StoreSqlite). Built only when SQLite is available.
find_package(SQLite3 QUIET)

if (SQLite3_FOUND)
    add_library(arcs_store_sqlite
        src/store_sqlite.cpp
    )
    target_include_directories(arcs_store_sqlite
        PUBLIC
            ${CMAKE_CURRENT_SOURCE_DIR}/include
    )
    set(ARCS_SQLITE_TARGET "")
    if (TARGET SQLite3::SQLite3)
        set(ARCS_SQLITE_TARGET SQLite3::SQLite3)
    elseif(TARGET SQLite::SQLite3)
        set(ARCS_SQLITE_TARGET SQLite::SQLite3)
    else()
        message(FATAL_ERROR "SQLite was found but no supported imported target exists")
    endif()

    target_link_libraries(arcs_store_sqlite
        PUBLIC
            arcs_store
            arcs_artifact
            arcs_event
            arcs_schema
            ${ARCS_SQLITE_TARGET}
    )
    set(ARCS_HAS_STORE_SQLITE TRUE CACHE BOOL "Build ARCS StoreSQLite" FORCE)
else()
    set(ARCS_HAS_STORE_SQLITE FALSE CACHE BOOL "Build ARCS StoreSQLite" FORCE)
endif()
