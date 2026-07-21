cmake_minimum_required(VERSION 3.16)
project(truega_lfm25_trace LANGUAGES C CXX)

if(NOT DEFINED LLAMA_SOURCE_DIR)
    message(FATAL_ERROR "LLAMA_SOURCE_DIR must point at the pinned llama.cpp b10075 checkout")
endif()

set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_COMMON OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_MTMD OFF CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_CPU_REPACK OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)

add_subdirectory("${LLAMA_SOURCE_DIR}" llama-b10075 EXCLUDE_FROM_ALL)

add_executable(truega-lfm25-trace llama_trace.cpp)
target_compile_features(truega-lfm25-trace PRIVATE cxx_std_17)
target_include_directories(truega-lfm25-trace PRIVATE
    "${LLAMA_SOURCE_DIR}/include"
    "${LLAMA_SOURCE_DIR}/ggml/include"
)
target_link_libraries(truega-lfm25-trace PRIVATE llama)

