cmake_minimum_required(VERSION 3.10)
project(baam-3ds)
# enable_language(ASM) # ASM must be enabled for add_shbin_library

set (CMAKE_CXX_STANDARD 17)

list(APPEND CMAKE_MODULE_PATH ${CMAKE_CURRENT_LIST_DIR}/cmake) # Add the cmake folder to the modules paths, so that we can use the tools and find_package for ctrulib
include(Tools3DS) # Include all the macros and tools needed for 3ds development.

find_package(CTRULIB REQUIRED) # Look for ctrulib

# add pqrs dep, disabling its binaries
set(BUILD_PQRS_BINS OFF CACHE BOOL "Build and install pqrs example binaries.")
add_subdirectory(ext-deps/pqrs)


# create a library that assembles and embeds the shaders shbin.
# add_shbin_library(shaders ${SHADERS_FILES})

add_library(baam-3ds-pqrs-wrapper STATIC source/processing.cpp)
target_compile_features(baam-3ds-pqrs-wrapper PUBLIC cxx_std_11)
target_link_libraries(baam-3ds-pqrs-wrapper pqrs 3ds::ctrulib m)

install(TARGETS baam-3ds-pqrs-wrapper DESTINATION lib/${CMAKE_LIBRARY_ARCHITECTURE})

# export library depen
include(cmake/export-libs.cmake)
export_all_target_libs(baam-3ds-pqrs-wrapper)
