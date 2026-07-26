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

fn unknown_confidence() -> Confidence {
    Confidence::Unknown
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
    /// 機械が存在することの確信度。レジストリから読めていれば `confirmed`
    pub confidence: Confidence,
    /// **読む/書くラベルの確信度。**機械の存在とは別に持つ。
    ///
    /// レジストリに機械があること(確定)と、その機械がどのラベルを扱うか(推測)は
    /// 別の主張であり、1つの確信度で表すと必ずどちらかを誤って表示する。
    #[serde(default = "unknown_confidence")]
    pub labels_confidence: Confidence,
    /// この機械の判断根拠になったファイル/APIレスポンス。詳細画面で表示する
    pub provenance: Vec<String>,
    pub summary: Option<String>,
    /// 担当リポジトリの手がかり。DesktopTaskの `cwd` 等
    pub working_dir: Option<String>,
}

/// そのラベルが工場のレーンだという証拠があるか。
///
/// 実リポジトリには工場と無関係なラベル(GitHubの標準ラベル等)が必ず混ざる。
/// ただし**証拠が無いことは無関係であることを意味しない。**断定すると推測に
/// なるため、あくまで「証拠の有無」として持つ。描画時に既定で畳むのは
/// `NoEvidence` 側だが、件数は常に見せること。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaneRelevance {
    /// 遷移が観測された、または機械の定義に名前が出てくる
    Factory,
    /// どちらの証拠も無い
    NoEvidence,
}

/// 滞留日数を何から出したか。
///
/// 「そのレーンに入った時刻」を観測できたかどうかで、数字の意味が変わる。
/// 同じ「5日」でも根拠の強さが違うので、区別せずに出してはならない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StagnationBasis {
    /// ラベルが付与された遷移を観測できた。これが本来の滞留
    Observed,
    /// 遷移が無く、issueの作成日時で代用した。
    /// そのレーンでissueが作られた場合や、遷移がAPIの取得範囲より古い場合
    IssueCreated,
}

/// レーン = ラベル1つ。コンベア上の在庫を表す。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lane {
    pub label: String,
    pub count: usize,
    /// 最古のissueが何日滞留しているか。ベルトの詰まりの指標
    pub oldest_days: Option<i64>,
    /// `oldest_days` を何から出したか。代用値を実測と同じ顔で出さないため
    #[serde(default)]
    pub oldest_basis: Option<StagnationBasis>,
    /// 最も滞留しているissueの番号。追跡の入口になる
    #[serde(default)]
    pub oldest_issue: Option<u64>,
    pub relevance: LaneRelevance,
    /// なぜ工場のレーンだと判断したか。詳細画面で表示する
    #[serde(default)]
    pub evidence: Vec<String>,
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
    /// 流入が観測されているのに流出が無いレーン。行き止まりの強い証拠
    OrphanLane { label: String, note: String },
    /// 在庫はあるが流入も流出も観測されていないレーン。
    ///
    /// issueがそのレーンで生まれた場合、行き止まりでも流入は記録されない。
    /// 行き止まりかもしれないし、単に動きが古いだけかもしれない。
    /// **OrphanLaneと同じ扱いにしてはならない**(証拠の強さが違う)
    UnobservedLane { label: String, note: String },
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
