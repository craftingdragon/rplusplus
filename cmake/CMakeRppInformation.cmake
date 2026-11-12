# SPDX-License-Identifier: GPL-3.0-or-later
# The first milestone emits no_std native objects, not general Rust staticlibs.
if(NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
  message(FATAL_ERROR "Rpp native-object integration is currently validated only on Linux")
endif()
set(CMAKE_Rpp_COMPILE_OBJECT "<CMAKE_Rpp_COMPILER> --edition=2024 --crate-type=lib --emit=obj,dep-info=<DEP_FILE> <FLAGS> -o <OBJECT> <SOURCE>")
set(CMAKE_Rpp_DEPFILE_FORMAT gcc)
set(CMAKE_Rpp_DEPENDS_USE_COMPILER TRUE)
set(CMAKE_Rpp_LINKER_PREFERENCE 10)
set(CMAKE_Rpp_ARCHIVE_CREATE "<CMAKE_AR> qc <TARGET> <OBJECTS>")
set(CMAKE_Rpp_ARCHIVE_APPEND "<CMAKE_AR> q <TARGET> <OBJECTS>")
set(CMAKE_Rpp_ARCHIVE_FINISH "<CMAKE_RANLIB> <TARGET>")
set(CMAKE_Rpp_INFORMATION_LOADED 1)
