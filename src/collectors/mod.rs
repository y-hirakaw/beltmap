//! 決定論コレクター(5.1)。
//!
//! 各コレクターは独立してテスト可能にする。副作用(コマンド実行・ファイル読み)と
//! パース処理を分け、パース側は fixture のバイト列だけで検証できるようにすること。

pub mod desktop_tasks;
pub mod labels;
pub mod routines;
pub mod transitions;
