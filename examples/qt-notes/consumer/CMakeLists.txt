# SPDX-License-Identifier: GPL-3.0-or-later
cmake_minimum_required(VERSION 3.25)
find_package(Rplusplus CONFIG REQUIRED)
project(NotesConsumer LANGUAGES CXX Rpp)
find_package(RppNotes CONFIG REQUIRED)
add_executable(NotesConsumer main.cpp)
target_link_libraries(NotesConsumer PRIVATE RppNotes::notes_backend)
enable_testing()
add_test(NAME installed-notes COMMAND NotesConsumer)
