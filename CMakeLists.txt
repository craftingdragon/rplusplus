cmake_minimum_required(VERSION 3.25)
project(Rplusplus VERSION 0.1.0 LANGUAGES NONE)
# No compiled binaries yet; keep installation independent of architecture discovery.
set(CMAKE_INSTALL_LIBDIR lib CACHE PATH "Library installation directory")
include(GNUInstallDirs)
install(DIRECTORY cmake/ DESTINATION "${CMAKE_INSTALL_DATADIR}/rplusplus/cmake")
install(DIRECTORY docs/ DESTINATION "${CMAKE_INSTALL_DOCDIR}")
install(FILES README.md LICENSE DESTINATION "${CMAKE_INSTALL_DOCDIR}")
find_program(RPP_RUSTC_EXECUTABLE NAMES rustc REQUIRED)
set(RPP_DRIVER "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp")
add_custom_command(OUTPUT "${RPP_DRIVER}"
  COMMAND "${CMAKE_COMMAND}" -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/bin"
  COMMAND "${RPP_RUSTC_EXECUTABLE}" --edition=2024 --emit=link,dep-info=${CMAKE_CURRENT_BINARY_DIR}/rpp.d
    "${CMAKE_CURRENT_SOURCE_DIR}/src/main.rs" -o "${RPP_DRIVER}"
  DEPENDS src/main.rs DEPFILE "${CMAKE_CURRENT_BINARY_DIR}/rpp.d" VERBATIM)
add_custom_target(rpp ALL DEPENDS "${RPP_DRIVER}")
install(PROGRAMS "${RPP_DRIVER}" DESTINATION "${CMAKE_INSTALL_BINDIR}")
find_package(Python3 COMPONENTS Interpreter)
include(CTest)
if(BUILD_TESTING)
  add_custom_command(OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp-tests"
    COMMAND "${RPP_RUSTC_EXECUTABLE}" --edition=2024 --test
      "${CMAKE_CURRENT_SOURCE_DIR}/src/main.rs" -o "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp-tests"
    DEPENDS src/main.rs rpp VERBATIM)
  add_custom_target(rpp-tests ALL DEPENDS "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp-tests")
  add_test(NAME driver-unit COMMAND "${CMAKE_CURRENT_BINARY_DIR}/bin/rpp-tests")
endif()
if(BUILD_TESTING AND Python3_FOUND)
  add_test(NAME generator-compatibility
    COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/generators.py")
endif()
