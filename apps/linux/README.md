# 青简输入法 Linux 版（fcitx5）

青简输入法的 Linux 前端，以 fcitx5 addon 形式运行。

## 架构

```
fcitx5 进程
├── libqingjian.so        ← C++ shim：实现 fcitx::InputMethodEngine
│   ├── engine.cpp        把 fcitx5 KeyEvent 翻译成 C ABI 调用
│   ├── factory.cpp       fcitx5 addon 工厂注册
│   └── abi.h             Rust cdylib 的 C ABI 声明
└── libqingjian_linux.so  ← Rust cdylib：装配 Engine、按键分派、生成帧
    ├── lib.rs            C ABI 导出（qingjian_init / qingjian_key_event / …）
    ├── assembly.rs       装配 Engine（词库 / 释义 / 学习 / 语言模型）
    ├── dispatch/         按键→Engine 分派（与 macOS / Windows 壳对齐）
    ├── session.rs        候选高亮 / 翻页状态
    ├── frame.rs          C ABI Frame（preedit + 候选表）
    └── paths.rs          XDG 路径 / 数据目录定位
```

Engine 在 fcitx5 进程内运行（与 macOS IMK 同进程模式一致），不需要 Windows 那套 IPC。
C++ shim 只做「fcitx5 事件 → C ABI → fcitx5 InputPanel」的翻译，不碰排序 / 词库 / 翻译。

## 依赖

- Rust 1.96.0（工作区 `rust-toolchain.toml` 指定）
- C++20 编译器
- fcitx5 开发包（Fedora: `fcitx5-devel`，Ubuntu/Debian: `libfcitx5core-dev`）
- CMake ≥ 3.16

## 构建

### 开发者：从源码构建安装

```sh
# 1. 编译 Rust cdylib
cargo build --release -p qingjian-linux

# 2. 编译 C++ shim
cmake -B apps/linux/shim/build -S apps/linux/shim -DCMAKE_BUILD_TYPE=Release
cmake --build apps/linux/shim/build
```

### 打包：生成通用安装包（用户不需要编译环境）

打包脚本会编译 `.so`、组装数据文件、打成 `.tar.gz`，附带安装 / 卸载脚本：

```sh
# 先下载产品数据（见「数据目录」），然后：
bash apps/linux/scripts/bundle.sh
# 产出 target/dist/qingjian-<版本>-linux-x86_64.tar.gz
```

用户拿到包后：

```sh
tar -xzf qingjian-*-linux-x86_64.tar.gz
cd qingjian-*-linux-x86_64
sudo ./install.sh          # 装到 /usr
fcitx5 -r                  # 重启 fcitx5
# 在 fcitx5 配置工具里把「青简」加到输入法列表
```

卸载：`sudo ./uninstall.sh`

包内含 `.so` + fcitx5 配置 + 词库 / 释义 / 语言模型，用户机器只要装了 fcitx5 就能用，
不需要 Rust / C++ / CMake。

## 安装

fcitx5 只扫描 `/usr/share/fcitx5` 和 `/usr/lib*/fcitx5`，不扫 `/usr/local`，
所以必须装到 `/usr`（用 `-DCMAKE_INSTALL_PREFIX=/usr`）：

```sh
cmake -B apps/linux/shim/build -S apps/linux/shim \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build apps/linux/shim/build
sudo cmake --install apps/linux/shim/build
```

安装内容：

| 文件 | 目标 | 作用 |
|------|------|------|
| `libqingjian.so` | `/usr/lib64/fcitx5/`（或 `/usr/lib/fcitx5/`） | C++ shim（fcitx5 addon） |
| `libqingjian_linux.so` | 同上 | Rust cdylib（Engine） |
| `qingjian.conf`（addon） | `/usr/share/fcitx5/addon/` | addon 描述（`Library=libqingjian`） |
| `qingjian.conf`（输入法） | `/usr/share/fcitx5/inputmethod/` | 输入法注册 |

> **注意**：addon conf 里的 `Library` 字段写的是带 `lib` 前缀的完整名（`libqingjian`），
> 与其他 fcitx5 addon 一致（如 rime 写 `Library=librime`）。

装完重启 fcitx5，然后在 fcitx5 配置工具（`fcitx5-configtool`）里把「青简」加到输入法列表：

```sh
fcitx5 -r
```

## 数据目录

青简的词库 / 释义 / 语言模型等数据文件按以下顺序查找（见 `paths.rs`）：

1. `QINGJIAN_DATA_DIR` 环境变量（开发时指向仓库根）
2. `/usr/share/qingjian`（安装路径）
3. 仓库根的 `data/` 与 `assets/`（开发时从 exe 往上找）

### 下载预生成数据

词库（`dict.qj`）、释义（`glossary-*.qj`）、语言模型（`lm.qj`）等预生成数据放在
GitHub release `data` 里，不在仓库中（太大）。开发时需要先下载：

```sh
# 下载并解压到 data/generated/（约 37 MB）
curl -L -o /tmp/qingjian-data.tar.gz \
    https://github.com/qingjian-team/qingjian/releases/download/data/qingjian-data.tar.gz
mkdir -p data/generated
tar -xzf /tmp/qingjian-data.tar.gz -C data/generated
```

解压后包含：

| 文件 | 说明 |
|------|------|
| `dict.qj` | 基础词库（打包格式，启动近零耗时） |
| `lm.qj` | 二元语言模型（整句排序用） |
| `glossary-zh.qj` / `glossary-en.qj` / `glossary-ja.qj` | 释义表 |
| `english.tsv` | 英文词表 |
| `dicts/` | 领域词库（额外词库） |

可选：神经整句模型（`model.qjm`，约 100 MB），提供更强的整句补全：

```sh
curl -L -o data/model/model.qjm \
    https://github.com/qingjian-team/qingjian/releases/download/data/model.qjm
```

### 开发时持久化 `QINGJIAN_DATA_DIR`

fcitx5 是后台进程，需要在会话级设环境变量。systemd 用户会话读 `~/.config/environment.d/`：

```sh
mkdir -p ~/.config/environment.d
echo "QINGJIAN_DATA_DIR=/path/to/qingjian" > ~/.config/environment.d/qingjian.conf
# 注销重新登录后生效
```

验证：`echo $QINGJIAN_DATA_DIR`。

### 正式安装

把数据目录装到 `/usr/share/qingjian`（`paths.rs` 已支持），就不需要环境变量：

```sh
sudo mkdir -p /usr/share/qingjian
sudo cp -r data/generated/* /usr/share/qingjian/
# 可选：神经模型
sudo cp data/model/model.qjm /usr/share/qingjian/model/
```

用户配置与学习数据在 `~/.config/qingjian/`（XDG）。

## 配置

配置文件在 `~/.config/qingjian/config.toml`，首次运行自动生成模板。

常用项：

```toml
[general]
# 双拼方案：留空为全拼；xiaohe 小鹤 / ziranma 自然码 / microsoft 微软 / sogou 搜狗
shuangpin = "xiaohe"
# 候选词数
page_size = 5
# 翻页键（默认 [ ]）
page_keys = "[]"
```

改完配置重启 fcitx5 生效：`fcitx5 -r`。

## 与其他平台壳的关系

| 平台 | 进程模型 | 前端 | 分派逻辑 |
|------|---------|------|---------|
| macOS | 同进程（IMK） | IMK | `apps/macos/src/imk/controller.rs` |
| Windows | IPC（服务器 + TSF） | TSF | `apps/windows/server/src/dispatch/key/` |
| Linux | 同进程（fcitx5 addon） | fcitx5 | `apps/linux/src/dispatch/key.rs` |

三个壳的分派逻辑对齐：平台层只做「把系统按键翻译成 Core 的输入，把 Core 返回的帧画出来」。

## 已知限制（MVP）

- 中英切换：MVP 阶段默认中文模式，未接 Caps Lock / fcitx5 中英切换机制（后续接）
- 自渲染候选窗、偏好设置面板、菜单、云预测、神经重排序：推迟
- 候选词的译文（释义）已通过 fcitx5 `CandidateWord::setComment` 显示在候选词右侧；序号用 `setLabels` 设为 1. 2. …，高亮用 `setCursorIndex`。样式受 fcitx5 面板主题限制，自绘候选窗推迟
