cmake_minimum_required(VERSION 3.10)
project(RTSP_Pusher)

set(CMAKE_CXX_STANDARD 11)

# 使用 pkg-config 查找 FFmpeg
find_package(PkgConfig REQUIRED)
pkg_check_modules(FFMPEG REQUIRED
    libavcodec
    libavformat
    libavfilter
    libavutil
    libswscale
    libswresample
)

# 包含目录
include_directories(${FFMPEG_INCLUDE_DIRS}
    ${CMAKE_CURRENT_SOURCE_DIR}/include
)

# 可执行文件
add_executable(rtsp_pusher rtsp_pusher.cpp)

# 链接库
target_link_libraries(rtsp_pusher
    ${FFMPEG_LIBRARIES}
)

# 可选：链接 pthread 等
target_link_libraries(rtsp_pusher pthread dl)