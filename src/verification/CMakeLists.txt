add_library(arcs_verification
    src/approval_verifier.cpp
    src/authority_verifier.cpp
    src/permission_verifier.cpp
    src/reference_integrity_verifier.cpp
    src/schema_verifier.cpp
    src/scope_verifier.cpp
    src/verification_engine.cpp
)

target_include_directories(arcs_verification
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_verification
    PUBLIC
        arcs_artifact
        arcs_reducer
        arcs_schema
        arcs_store
        nlohmann_json::nlohmann_json
)
