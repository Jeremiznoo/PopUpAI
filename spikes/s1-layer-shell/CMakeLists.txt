cmake_minimum_required(VERSION 3.21)
project(popupai-spike-s1 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Quick)
find_package(LayerShellQt REQUIRED)

qt_add_executable(spike_s1 main.cpp)
qt_add_resources(spike_s1 "qml" PREFIX "/" FILES Spike.qml)
target_link_libraries(spike_s1 PRIVATE Qt6::Core Qt6::Gui Qt6::Quick LayerShellQt::Interface)
target_compile_options(spike_s1 PRIVATE -Wall -Wextra)
