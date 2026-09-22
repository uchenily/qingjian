//! 候选窗口的排布：本地候选按页排，固定位置的自定义短语按位置放。
//!
//! 这是展示规则不是排序规则，但 CLI 与各平台壳都要用同一套，所以放在 Core。

mod candidate_layout;
mod cell;

pub use candidate_layout::CandidateLayout;
pub use cell::Cell;
