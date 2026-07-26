//! `.beltmap/config.toml`。ユーザーが手で書く唯一のファイル(設計原則1)。
//!
//! 工場の定義そのものはここには書かない。書くのは「どこから情報を取るか」だけ。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// issue/ラベルがあるリポジトリ。定義リポジトリとは別でありうる(5.4)。
    /// 決定論で解決できなかった場合にユーザーへ聞き、その結果がここに永続化される
    pub state_repo: Option<String>,

    /// ラッパースクリプト置き場。リポジトリルートからの相対パス
    #[serde(default)]
    pub script_dirs: Vec<String>,

    /// レーンの滞留を警告色にする閾値(日)
    #[serde(default = "default_stale_days")]
    pub stale_days: i64,

    #[serde(default)]
    pub collectors: CollectorToggles,

    /// クラウドルーチンAPIのトークン。未設定ならroutinesコレクターは
    /// 「取得不可」として明示表示し、他の機能は動かす(5.3)
    pub routines_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorToggles {
    #[serde(default = "yes")]
    pub labels: bool,
    #[serde(default = "yes")]
    pub claude_dir: bool,
    #[serde(default = "yes")]
    pub routines: bool,
    #[serde(default = "yes")]
    pub desktop_tasks: bool,
    #[serde(default = "yes")]
    pub scripts: bool,
    #[serde(default = "yes")]
    pub actions: bool,
    #[serde(default = "yes")]
    pub transitions: bool,
}

fn yes() -> bool {
    true
}

fn default_stale_days() -> i64 {
    3
}

impl Default for CollectorToggles {
    fn default() -> Self {
        Self {
            labels: true,
            claude_dir: true,
            routines: true,
            desktop_tasks: true,
            scripts: true,
            actions: true,
            transitions: true,
        }
    }
}
