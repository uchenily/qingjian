use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// `.qj` 里的一条释义：译文、读音、词性缩写都是 arena 里的一段（长度 0 表示没有）。20 字节、无填充。
///
/// 读音只服务日语假名注音，只支持中文 / 英文后新表恒为 0/0；字段留着不动，`.qj` 格式版本就不用升。
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct SenseRecord {
    /// 译文起点。
    pub text_start: u32,

    /// 读音起点（保留，恒为 0）。
    pub reading_start: u32,

    /// 词性缩写起点。
    pub pos_start: u32,

    /// 译文长度。
    pub text_len: u16,

    /// 读音长度（保留，恒为 0）。
    pub reading_len: u16,

    /// 词性缩写长度。
    pub pos_len: u16,

    /// 对齐用，全零。
    pub reserved: u16,
}
