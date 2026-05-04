cmake_minimum_required(VERSION 3.20)
project(arcs_ingress LANGUAGES CXX)

add_library(arcs_ingress
    src/ingress_normalizer.cpp
    src/ingress_validator.cpp
    src/ingress_router.cpp
    src/quarantine.cpp
)

target_include_directories(arcs_ingress
    PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_link_libraries(arcs_ingress
    PUBLIC
        arcs_artifact
        arcs_schema
)

target_compile_features(arcs_ingress PRIVATE cxx_std_20)
