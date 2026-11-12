# SPDX-License-Identifier: GPL-3.0-or-later
if(NOT CMAKE_Rpp_COMPILER_WORKS)
  file(WRITE "${CMAKE_BINARY_DIR}/CMakeFiles/rpp-compiler-probe.rs" "#![no_std]\n#[unsafe(no_mangle)] pub extern \"C\" fn probe() -> i32 { 0 }\n")
  set(_rpp_saved_try_compile_type "${CMAKE_TRY_COMPILE_TARGET_TYPE}")
  set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
  try_compile(CMAKE_Rpp_COMPILER_WORKS
    SOURCES "${CMAKE_BINARY_DIR}/CMakeFiles/rpp-compiler-probe.rs"
    OUTPUT_VARIABLE probe_output)
  set(CMAKE_TRY_COMPILE_TARGET_TYPE "${_rpp_saved_try_compile_type}")
  unset(_rpp_saved_try_compile_type)
  message(STATUS "Rpp compiler check=${CMAKE_Rpp_COMPILER_WORKS}")
  if(NOT CMAKE_Rpp_COMPILER_WORKS)
    message(FATAL_ERROR "Rpp compiler check failed: ${probe_output}")
  endif()
endif()
