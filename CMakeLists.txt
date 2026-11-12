cmake_minimum_required(VERSION 3.25)
project(Rplusplus VERSION 0.1.0 LANGUAGES NONE)
# No compiled binaries yet; keep installation independent of architecture discovery.
set(CMAKE_INSTALL_LIBDIR lib CACHE PATH "Library installation directory")
include(GNUInstallDirs)
install(DIRECTORY cmake/ DESTINATION "${CMAKE_INSTALL_DATADIR}/rplusplus/cmake")
install(FILES README.md LICENSE DESTINATION "${CMAKE_INSTALL_DOCDIR}")
find_package(Python3 COMPONENTS Interpreter)
include(CTest)
if(BUILD_TESTING AND Python3_FOUND)
  add_test(NAME generator-compatibility
    COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/tests/generators.py")
endif()
