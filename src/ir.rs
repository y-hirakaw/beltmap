//! 工場のグラフ表現(IR)。
//!
//! IRは導出物であってユーザーの編集対象ではない(設計原則2)。
//! スキャンのたびに作り直され、`.beltmap/ir.json` に保存される。

use serde::{Deserialize, Serialize};

pub const IR_VERSION: u32 = 1;

/// 判断の確からしさ。実測とAI推測を混ぜて表示しないための区別(設計原則4)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// 実測、またはユーザーが確認済み
    Confirmed,
    /// AI推測。TUIでは点線で描く
    Inferred,
    /// 埋まっていない。`?` で描く
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MachineStatus {
    /// 遷移実績がある
    Active,
    /// 定義はあるが遷移実績がない
    Building,
    /// 参照されているだけで定義が見つからない
    Planned,
}

/// 機械がどこで動くか。実行条件が違うため区別する(5.3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    /// Anthropicクラウドで動く。アカウント全体から見える
    CloudRoutine,
    /// このマシンでのみ動き、このマシンからしか見えない
    DesktopTask,
    GithubActions,
    /// 痕跡から存在は分かるが実体を特定できていない
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Trigger {
    Schedule { detail: String },
    Event { detail: String },
    Manual,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub runtime: Runtime,
    pub trigger: Trigger,
    /// 読むラベル。決定論で埋まらない場合が多く、その場合は推測層が埋める
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub status: MachineStatus,
    pub confidence: Confidence,
    /// この機械の判断根拠になったファイル/APIレスポンス。詳細画面で表示する
    pub provenance: Vec<String>,
    pub summary: Option<String>,
    /// 担当リポジトリの手がかり。DesktopTaskの `cwd` 等
    pub working_dir: Option<String>,
}

/// レーン = ラベル1つ。コンベア上の在庫を表す。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lane {
    pub label: String,
    pub count: usize,
    /// 最古のissueが何日滞留しているか。ベルトの詰まりの指標
    pub oldest_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub condition: Option<String>,
    pub confidence: Confidence,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Unknown {
    /// 消費する機械が見つからないラベル
    OrphanLane { label: String, note: String },
    /// クラウド側にはあるが、対応するローカル機械がこのマシンに無い。
    /// 別マシンに機械がある強い証拠になる(5.3)
    MachineNotOnThisHost { machine_id: String, note: String },
    Other { note: String },
}

/// 確認モードでユーザーが答えた内容。再スキャンしても再質問しないために永続化する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub question_hash: String,
    pub answer: String,
    pub answered_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ir {
    pub version: u32,
    pub scanned_at: chrono::DateTime<chrono::Utc>,
    /// どのマシンでスキャンしたか。地図はマシン依存なので記録が要る(5.3)
    pub scanned_on: String,
    pub machines: Vec<Machine>,
    pub lanes: Vec<Lane>,
    pub edges: Vec<Edge>,
    pub unknowns: Vec<Unknown>,
    pub answers: Vec<Answer>,
}
