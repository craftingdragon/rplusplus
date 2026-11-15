cmake_minimum_required(VERSION 3.25)
project(Rplusplus VERSION 0.1.0 LANGUAGES NONE)
# Driver bootstrap uses rustc directly without enabling a compiled CMake language.
set(CMAKE_INSTALL_LIBDIR lib CACHE PATH "Library installation directory")
include(GNUInstallDirs)
install(DIRECTORY cmake/ DESTINATION "${CMAKE_INSTALL_DATADIR}/rplusplus/cmake")
install(DIRECTORY docs/ DESTINATION "${CMAKE_INSTALL_DOCDIR}")
install(FILES README.md LICENSE DESTINATION "${CMAKE_INSTALL_DOCDIR}")
install(DIRECTORY LICENSES/ DESTINATION "${CMAKE_INSTALL_DOCDIR}/licenses")
find_program(RPP_RUSTC_EXECUTABLE NAMES rustc REQUIRED)
include("${CMAKE_CURRENT_SOURCE_DIR}/cmake/RppToolchain.cmake")
rpp_validate_toolchain()
set(RPP_DRIVER "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp")
set(RPP_DRIVER_SOURCES src/main.rs src/lex/mod.rs src/lex/scanner.rs src/lex/unicode.rs)
# A dep-info-only pass gives one stable depfile target across rustc versions.
# Combined link+dep-info changes target order between supported compilers, which
# CMake 3.25's Ninja depfile handling cannot accept. The raw .d is the primary
# output; CMake consumes a separate transformed .d, not this raw one.
set(_rpp_driver_outputs "${CMAKE_CURRENT_BINARY_DIR}/rpp.d" "${RPP_DRIVER}")
set(_rpp_test_outputs "${CMAKE_CURRENT_BINARY_DIR}/rpp-tests.d" "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp-tests")
add_custom_command(OUTPUT ${_rpp_driver_outputs}
  COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/bin" "${CMAKE_CURRENT_BINARY_DIR}/.rpp-bootstrap/driver"
  # rustc does not escape spaces in depfile target names. Relative output names
  # avoid malformed rules when the build directory contains spaces or Unicode.
  COMMAND "${CMAKE_COMMAND}" -E chdir "${CMAKE_CURRENT_BINARY_DIR}/.rpp-bootstrap/driver"
    "${RPP_RUSTC_EXECUTABLE}" --edition=2024 --emit=dep-info=rpp.d
    "${CMAKE_CURRENT_SOURCE_DIR}/src/main.rs"
  COMMAND "${RPP_RUSTC_EXECUTABLE}" --edition=2024 --emit=link
    "${CMAKE_CURRENT_SOURCE_DIR}/src/main.rs" -o .rpp-bootstrap/driver/rpp
  COMMAND "${CMAKE_COMMAND}" -E rename .rpp-bootstrap/driver/rpp bin/rpp
  COMMAND "${CMAKE_COMMAND}" -E rename .rpp-bootstrap/driver/rpp.d rpp.d
  WORKING_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}"
  DEPENDS ${RPP_DRIVER_SOURCES} DEPFILE "${CMAKE_CURRENT_BINARY_DIR}/rpp.d" VERBATIM)
add_custom_target(rpp ALL DEPENDS ${_rpp_driver_outputs})
install(PROGRAMS "${RPP_DRIVER}" DESTINATION "${CMAKE_INSTALL_BINDIR}")
find_package(Python3 COMPONENTS Interpreter)
include(CTest)
if(BUILD_TESTING)
  add_custom_command(OUTPUT ${_rpp_test_outputs}
    COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/.rpp-bootstrap/tests"
    COMMAND "${CMAKE_COMMAND}" -E chdir "${CMAKE_CURRENT_BINARY_DIR}/.rpp-bootstrap/tests"
      "${RPP_RUSTC_EXECUTABLE}" --edition=2024 --test
      --emit=dep-info=rpp-tests.d "${CMAKE_CURRENT_SOURCE_DIR}/src/main.rs"
    COMMAND "${RPP_RUSTC_EXECUTABLE}" --edition=2024 --test --emit=link
      "${CMAKE_CURRENT_SOURCE_DIR}/src/main.rs" -o .rpp-bootstrap/tests/rpp-tests
    COMMAND "${CMAKE_COMMAND}" -E rename .rpp-bootstrap/tests/rpp-tests bin/rpp-tests
    COMMAND "${CMAKE_COMMAND}" -E rename .rpp-bootstrap/tests/rpp-tests.d rpp-tests.d
    WORKING_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}"
    DEPENDS ${RPP_DRIVER_SOURCES} src/lex/tests.rs rpp
    DEPFILE "${CMAKE_CURRENT_BINARY_DIR}/rpp-tests.d" VERBATIM)
  add_custom_target(rpp-tests ALL DEPENDS ${_rpp_test_outputs})
  add_test(NAME driver-unit COMMAND "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp-tests")
endif()
set_property(DIRECTORY APPEND PROPERTY ADDITIONAL_CLEAN_FILES "${CMAKE_CURRENT_BINARY_DIR}/.rpp-bootstrap")
if(BUILD_TESTING AND Python3_FOUND)
  add_test(NAME driver-failures COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/driver_failures.py" "${RPP_DRIVER}")
  add_test(NAME toolchain-errors COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/toolchain_errors.py")
  add_test(NAME lexer-regression COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/lexer.py")
  add_test(NAME generator-compatibility
    COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/generators.py")
endif()

option(RPP_ENABLE_QT_VALIDATION "Require system Qt Widgets and SQLite validation" OFF)
if(BUILD_TESTING AND RPP_ENABLE_QT_VALIDATION)
  if(NOT Python3_FOUND)
    message(FATAL_ERROR "Qt validation requires Python3")
  endif()
  add_test(NAME qt-notes-installed COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/qt_notes.py")
endif()
