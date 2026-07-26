//! 推測層との往復ファイルの契約(5.2)。
//!
//! beltmapはLLMを呼ばない。埋まらなかった穴を `enrichment-request.json` に
//! 書き出し、ユーザーのClaudeセッションが `beltmap-enrich` skillで
//! `enrichment.json` を返す。beltmapは検証してから取り込む。

use serde::{Deserialize, Serialize};

/// 埋めてほしい穴1件。「質問」ではなく根拠つきのタスクとして渡す。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentTask {
    pub machine_id: String,
    /// 根拠になるソース本文(SKILL.md本文・ルーチンプロンプト・スクリプト内容)
    pub source_text: String,
    /// `source_text` のハッシュ。ソースが変われば推測を無効化するため(5.2)
    pub source_hash: String,
    /// 埋めてほしいフィールド名
    pub fields: Vec<String>,
    /// 実在するラベル一覧。存在しないラベルを創作させないための制約
    pub known_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentRequest {
    pub version: u32,
    pub tasks: Vec<EnrichmentTask>,
}

/// skillが返す推測1件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentResult {
    pub machine_id: String,
    /// 依頼時の `source_hash` をそのまま返させる。
    /// 現在のソースと一致しなければ陳腐化とみなし `unknown` に戻す
    pub source_hash: String,
    #[serde(default)]
    pub reads: Vec<String>,
    #[serde(default)]
    pub writes: Vec<String>,
    pub summary: Option<String>,
    pub trigger_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentResponse {
    pub version: u32,
    pub results: Vec<EnrichmentResult>,
}
