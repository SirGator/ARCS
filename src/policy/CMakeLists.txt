add_library(arcs_policy
    src/policy_artifact.cpp
    src/permission_grant.cpp
)

target_include_directories(arcs_policy
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_policy
    PUBLIC
        nlohmann_json::nlohmann_json
)
