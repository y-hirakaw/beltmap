//! 推測層との往復ファイルの契約(5.2)。
//!
//! beltmapはLLMを呼ばない。埋まらなかった穴を `enrichment-request.json` に
//! 書き出し、ユーザーのClaudeセッションが `beltmap-enrich` skillで
//! `enrichment.json` を返す。beltmapは検証してから取り込む。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ENRICH_VERSION: u32 = 1;

/// 根拠テキストのハッシュ。
///
/// 永続化して次回スキャンと突き合わせるため、プロセス間・バージョン間で
/// 安定した値でなければならない。`DefaultHasher` は安定を保証しないので
/// 使えない。
pub fn source_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

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

/// 取り込み時の判定結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 取り込む
    Accept,
    /// 根拠が変わっている。推測は無効
    Stale,
    /// 実在しないラベルを含む。取り込むと幻覚コンベアになる
    UnknownLabel(String),
    /// 依頼していない機械への回答
    NotRequested,
}

/// skillの回答を1件検証する。
///
/// **検証を通ったものだけを取り込む。**モデルが指示に従う前提のコードを
/// 書いてはならない(実測でコードフェンス禁止を指示しても包んで返した)。
/// 落ちた項目はその項目だけ捨て、ファイル全体は捨てない。
pub fn verify(result: &EnrichmentResult, request: &EnrichmentRequest) -> Verdict {
    let Some(task) = request
        .tasks
        .iter()
        .find(|t| t.machine_id == result.machine_id)
    else {
        return Verdict::NotRequested;
    };

    if task.source_hash != result.source_hash {
        return Verdict::Stale;
    }

    for label in result.reads.iter().chain(result.writes.iter()) {
        if !task.known_labels.contains(label) {
            return Verdict::UnknownLabel(label.clone());
        }
    }

    Verdict::Accept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> EnrichmentTask {
        EnrichmentTask {
            machine_id: "desktop:triage".into(),
            source_text: "本文".into(),
            source_hash: source_hash("本文"),
            fields: vec!["reads".into(), "writes".into()],
            known_labels: vec!["ai-process:ready".into(), "ai-process:spec-draft".into()],
        }
    }

    fn request() -> EnrichmentRequest {
        EnrichmentRequest {
            version: ENRICH_VERSION,
            tasks: vec![task()],
        }
    }

    fn result(reads: Vec<&str>, hash: &str) -> EnrichmentResult {
        EnrichmentResult {
            machine_id: "desktop:triage".into(),
            source_hash: hash.into(),
            reads: reads.into_iter().map(String::from).collect(),
            writes: vec![],
            summary: None,
            trigger_detail: None,
        }
    }

    #[test]
    fn hash_is_stable_for_the_same_text() {
        assert_eq!(source_hash("同じ本文"), source_hash("同じ本文"));
        assert_ne!(source_hash("本文A"), source_hash("本文B"));
    }

    #[test]
    fn accepts_a_well_formed_answer() {
        let r = result(vec!["ai-process:ready"], &source_hash("本文"));
        assert_eq!(verify(&r, &request()), Verdict::Accept);
    }

    #[test]
    fn rejects_answer_whose_source_changed() {
        // SKILL.mdを書き換えたのに古い推測が残ると、実線同然の顔で嘘が居座る
        let r = result(vec!["ai-process:ready"], &source_hash("書き換えた本文"));
        assert_eq!(verify(&r, &request()), Verdict::Stale);
    }

    #[test]
    fn rejects_labels_that_do_not_exist() {
        // 実在しないラベルを地図に描くと幻覚コンベアそのものになる
        let r = result(vec!["ai-process:sounds-plausible"], &source_hash("本文"));
        assert!(matches!(
            verify(&r, &request()),
            Verdict::UnknownLabel(l) if l == "ai-process:sounds-plausible"
        ));
    }

    #[test]
    fn rejects_answers_for_machines_we_did_not_ask_about() {
        let mut r = result(vec![], &source_hash("本文"));
        r.machine_id = "desktop:invented".into();
        assert_eq!(verify(&r, &request()), Verdict::NotRequested);
    }
}
