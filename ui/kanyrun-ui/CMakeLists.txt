cmake_minimum_required(VERSION 3.16)
project(kanyrun-ui LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Widgets DBus)

set(LAYERSHELLQT_INCLUDE_HINT "${CMAKE_CURRENT_SOURCE_DIR}/../../.cache/layershellqt-devel-x86_64/usr/include")
find_path(LAYERSHELLQT_INCLUDE_DIR
    NAMES LayerShellQt/window.h
    PATHS "${LAYERSHELLQT_INCLUDE_HINT}"
    NO_DEFAULT_PATH
)
if(NOT LAYERSHELLQT_INCLUDE_DIR)
    message(FATAL_ERROR "LayerShellQt headers not found. Download or install layer-shell-qt-devel first.")
endif()

find_library(LAYERSHELLQT_INTERFACE_LIB
    NAMES LayerShellQtInterface libLayerShellQtInterface.so.6
    PATHS /usr/lib64
    NO_DEFAULT_PATH
)
if(NOT LAYERSHELLQT_INTERFACE_LIB)
    message(FATAL_ERROR "LayerShellQt runtime library not found.")
endif()

qt_standard_project_setup()

qt_add_executable(kanyrun-ui
    main.cpp
)

target_include_directories(kanyrun-ui
    PRIVATE ${LAYERSHELLQT_INCLUDE_DIR}
)

target_link_libraries(kanyrun-ui
    PRIVATE Qt6::Core Qt6::Gui Qt6::Widgets Qt6::DBus ${LAYERSHELLQT_INTERFACE_LIB}
)
