//! 数据文件位置：只读数据在安装目录下，用户数据在 `~/.config/qingjian/`（XDG）。

use std::path::PathBuf;

/// 随包只读数据的根目录：其下有 `data/` 与 `assets/`。
///
/// 三套布局，按优先级：
/// 1. 环境变量 `QINGJIAN_DATA_DIR`（开发时指向仓库根）。
/// 2. 安装目录：fcitx5 addon 装在 `/usr/lib/fcitx5/` 或 `~/.local/share/fcitx5/`，数据装在
///    `/usr/share/qingjian/` 或 `~/.local/share/qingjian/`。这里按 `bundled_root()` 找。
/// 3. 开发布局：仓库根（exe 在 `target/{debug,release}` 下，往上三层）。
pub fn data_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("QINGJIAN_DATA_DIR") {
        let dir = PathBuf::from(dir);
        if has_resources(&dir) {
            return Some(dir);
        }
    }
    if let Some(dir) = installed_data_dir() {
        return Some(dir);
    }
    dev_root()
}

/// 安装目录下的数据：`/usr/share/qingjian` 与 `~/.local/share/qingjian`。
fn installed_data_dir() -> Option<PathBuf> {
    for dir in ["/usr/share/qingjian", "/usr/local/share/qingjian"] {
        let dir = PathBuf::from(dir);
        if has_resources(&dir) {
            return Some(dir);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let dir = PathBuf::from(home).join(".local/share/qingjian");
        if has_resources(&dir) {
            return Some(dir);
        }
    }
    None
}

/// 开发布局：仓库根（exe → {debug,release} → target → apps/linux → 仓库根）。
fn dev_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dev_root = exe.ancestors().nth(3)?;
    has_resources(dev_root).then(|| dev_root.to_path_buf())
}

/// 一个目录是不是数据根：有 `data` 或 `assets` 子目录就算。
fn has_resources(dir: &std::path::Path) -> bool {
    dir.join("data").is_dir() || dir.join("assets").is_dir()
}

/// 某个资源文件的完整路径（相对数据根，如 `data/generated/dict.qj`）；不存在为 `None`。
pub fn resource(rel: &str) -> Option<PathBuf> {
    data_root().and_then(|root| {
        let path = root.join(rel);
        path.is_file().then_some(path)
    })
}

/// 用户数据目录：`~/.config/qingjian/`（遵循 XDG，缺省 `~/.config`）。不存在则创建。
pub fn user_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let dir = base.join("qingjian");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 配置文件：`~/.config/qingjian/config.toml`。
pub fn config_file() -> Option<PathBuf> {
    user_dir().map(|dir| dir.join("config.toml"))
}

/// 随包的领域词库目录：`<data根>/data/generated/dicts/`；没有为 `None`。
pub fn bundled_dicts_dir() -> Option<PathBuf> {
    data_root().and_then(|root| {
        let dir = root.join("data/generated/dicts");
        dir.is_dir().then_some(dir)
    })
}

/// 用户导入词库目录：`~/.config/qingjian/dicts/`，不存在则创建。
pub fn user_dicts_dir() -> Option<PathBuf> {
    let dir = user_dir()?.join("dicts");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 日志目录：`~/.config/qingjian/logs/`。
pub fn log_dir() -> Option<PathBuf> {
    let dir = user_dir()?.join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}
