cmake_minimum_required(VERSION 3.20)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

project(ARCS VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

include(FetchContent)

find_package(nlohmann_json CONFIG QUIET)

if (NOT TARGET nlohmann_json::nlohmann_json)
    set(JSON_BuildTests OFF CACHE INTERNAL "" FORCE)
    FetchContent_Declare(nlohmann_json
        URL https://github.com/nlohmann/json/releases/download/v3.11.3/json.tar.xz
    )
    FetchContent_MakeAvailable(nlohmann_json)
endif()

FetchContent_Declare(valijson
    GIT_REPOSITORY https://github.com/tristanpenman/valijson.git
    GIT_TAG v1.1.0
    GIT_SHALLOW TRUE
    GIT_SUBMODULES ""
)

set(valijson_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(valijson_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
FetchContent_MakeAvailable(valijson)

include(CTest)

add_subdirectory(src/artifact)
add_subdirectory(src/event)
add_subdirectory(src/schema)
add_subdirectory(app)

if(BUILD_TESTING)
    add_subdirectory(tests)
endif()
