# 青简输入法 fcitx5 addon 构建脚本。
#
# 构建两样东西：
# 1. Rust cdylib（libqingjian_core.so）：由 cargo 构建，这里作为 IMPORTED 目标引用
# 2. C++ shim（libqingjian.so）：实现 fcitx5 InputMethodEngine，链接 Rust cdylib
#
# 安装到 fcitx5 的目录（与系统其他 addon 一致）：
# - libqingjian.so（C++ shim）→ ${FCITX_INSTALL_ADDONDIR}（/usr/lib64/fcitx5）
# - libqingjian_core.so（Rust cdylib）→ 同上（靠 $ORIGIN RPATH 找到）
# - qingjian.conf（addon 描述）→ ${FCITX_INSTALL_PKGDATADIR}/addon（/usr/share/fcitx5/addon）
# - qingjian.conf（输入法注册）→ ${FCITX_INSTALL_PKGDATADIR}/inputmethod
#
# 用法：
#   cargo build --release -p qingjian-linux
#   cmake -B build -S apps/linux/shim -DCMAKE_INSTALL_PREFIX=/usr
#   cmake --build build
#   sudo cmake --install build
cmake_minimum_required(VERSION 3.16)
project(qingjian-fcitx5 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# fcitx5 包（提供 FCITX_INSTALL_* 变量和导入目标）
find_package(Fcitx5Core REQUIRED)
find_package(Fcitx5Utils REQUIRED)

# Rust cdylib：假设已用 cargo build --release 构建，产物在仓库根的 target/release/。
# 用户也可用 -DRUST_LIB_DIR=/path/to/lib 指定。
get_filename_component(REPO_ROOT "${CMAKE_SOURCE_DIR}/../../.." ABSOLUTE)
set(RUST_LIB_DIR "${REPO_ROOT}/target/release" CACHE PATH "Rust cdylib 输出目录")
set(RUST_LIB "${RUST_LIB_DIR}/libqingjian_linux.so")

if(NOT EXISTS "${RUST_LIB}")
    message(WARNING
        "找不到 Rust cdylib ${RUST_LIB}。\n"
        "请先在仓库根目录运行：cargo build --release -p qingjian-linux")
endif()

# C++ shim。fcitx5 addon 的 .so 文名要 lib<addonname>.so，这里 addon 名是 qingjian。
add_library(qingjian MODULE
    engine.cpp
    factory.cpp)
set_target_properties(qingjian PROPERTIES
    OUTPUT_NAME qingjian
    PREFIX "lib")
target_include_directories(qingjian PRIVATE
    ${CMAKE_SOURCE_DIR})
target_link_libraries(qingjian PRIVATE Fcitx5::Core)
target_link_libraries(qingjian PRIVATE "${RUST_LIB}")
# 让 libqingjian.so 运行时能找到 libqingjian_linux.so：靠 $ORIGIN RPATH
# 直接用链接器标志写死 $ORIGIN，并跳过 CMake 自动加的构建机绝对路径 RPATH。
set_target_properties(qingjian PROPERTIES
    SKIP_BUILD_RPATH ON
    INSTALL_RPATH "$ORIGIN"
    LINK_FLAGS "-Wl,-rpath,\$ORIGIN")

# 安装
install(TARGETS qingjian
    LIBRARY DESTINATION ${FCITX_INSTALL_ADDONDIR}
    NAMELINK_SKIP)

# Rust cdylib：装到 addon 目录（与 libqingjian.so 同目录，靠 $ORIGIN RPATH 找到）
install(FILES "${RUST_LIB}"
    DESTINATION ${FCITX_INSTALL_ADDONDIR}
    OPTIONAL)

# addon 描述（fcitx5 加载 addon 的清单）
install(FILES qingjian-addon.conf
    DESTINATION ${FCITX_INSTALL_PKGDATADIR}/addon
    RENAME qingjian.conf)

# 输入法注册（让 fcitx5 配置工具里能选到「青简」）
install(FILES qingjian.conf
    DESTINATION ${FCITX_INSTALL_PKGDATADIR}/inputmethod)
