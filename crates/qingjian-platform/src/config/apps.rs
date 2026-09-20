use serde::{Deserialize, Serialize};

/// 配置文件 `[apps]` 分节：按应用改行为。应用的标识 macOS 上是 bundle identifier，Windows 上是宿主进程的 exe 文件名。
///
/// 英文模式已改为彻底直通（不组句、不出候选、不转全角），按应用关闭候选的名单随之移除。
/// 以后按应用定 preedit 模式等仍放这里。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppsConfig {}
