# SPDX-License-Identifier: GPL-3.0-or-later
include(CMakeFindDependencyMacro)
find_dependency(SQLite3)
# CMake 3.25 exposes only the legacy imported name.
if(NOT TARGET SQLite3::SQLite3)
  add_library(SQLite3::SQLite3 ALIAS SQLite::SQLite3)
endif()
include("${CMAKE_CURRENT_LIST_DIR}/RppNotesTargets.cmake")
