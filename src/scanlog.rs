//! スキャンの記録。`.beltmap/scan-log.jsonl` に1スキャン1行で追記する。
//!
//! 目的はクラッシュ調査ではなく、**何をどこから取れて何が取れなかったか**を
//! 後から検分できるようにすること。工場マシンで動かした結果を持ち帰って
//! 改善の材料にする用途を想定している。
//!
//! 1行1レポートのJSONLにしているのは、スキャンを重ねると差分が工場の
//! ドリフトそのものになるため。上書きすると前回との比較が失われる。

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const SCANLOG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectorStatus {
    Ok,
    /// 設定で無効化された、または前提が満たされない(`gh`が無い等)
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorReport {
    pub name: String,
    /// 具体的に何を読んだか。パス・エンドポイント・実行したコマンド
    pub source: String,
    pub status: CollectorStatus,
    pub items: usize,
    pub duration_ms: u64,
    /// 失敗理由・スキップ理由。`Ok` 以外では必ず入る
    pub note: Option<String>,
}

impl CollectorReport {
    pub fn ok(name: &str, source: &str, items: usize, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            source: source.to_string(),
            status: CollectorStatus::Ok,
            items,
            duration_ms,
            note: None,
        }
    }

    /// 失敗は理由なしに記録できない。
    ///
    /// 「取れなかったこと」を黙って落とすのが最悪の失敗モードであり
    /// (5.3)、その防止をシグネチャで強制している。
    pub fn failed(name: &str, source: &str, reason: &str) -> Self {
        Self {
            name: name.to_string(),
            source: source.to_string(),
            status: CollectorStatus::Failed,
            items: 0,
            duration_ms: 0,
            note: Some(reason.to_string()),
        }
    }

    pub fn skipped(name: &str, source: &str, reason: &str) -> Self {
        Self {
            name: name.to_string(),
            source: source.to_string(),
            status: CollectorStatus::Skipped,
            items: 0,
            duration_ms: 0,
            note: Some(reason.to_string()),
        }
    }
}

/// 決定論で埋まらなかった穴。推測層に渡す前の生の状態を記録する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub machine_id: String,
    pub fields: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub version: u32,
    pub beltmap_version: String,
    /// どのマシンでスキャンしたか。地図はマシン依存なので必須(5.3)
    pub scanned_on: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub duration_ms: u64,
    pub collectors: Vec<CollectorReport>,
    pub machines: usize,
    pub lanes: usize,
    pub transitions: usize,
    pub unknowns: Vec<String>,
    pub gaps: Vec<Gap>,
    /// 推測層の回答のうち検証で弾いたもの。黙って捨てた記録を残す
    #[serde(default)]
    pub rejected: Vec<String>,
}

impl ScanReport {
    pub fn new(started_at: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            version: SCANLOG_VERSION,
            beltmap_version: env!("CARGO_PKG_VERSION").to_string(),
            scanned_on: hostname(),
            started_at,
            duration_ms: 0,
            collectors: Vec::new(),
            machines: 0,
            lanes: 0,
            transitions: 0,
            unknowns: Vec::new(),
            gaps: Vec::new(),
            rejected: Vec::new(),
        }
    }

    /// 取れなかったコレクター。TUIで明示表示する対象でもある。
    pub fn problems(&self) -> Vec<&CollectorReport> {
        self.collectors
            .iter()
            .filter(|c| c.status != CollectorStatus::Ok)
            .collect()
    }

    /// 人が読む用の1行要約。工場マシンでの実行直後に端末へ出す。
    pub fn summary_line(&self) -> String {
        let problems = self.problems().len();
        format!(
            "機械 {} / レーン {} / 遷移 {} / 未解決 {} / 問題のあるコレクター {}",
            self.machines,
            self.lanes,
            self.transitions,
            self.unknowns.len(),
            problems
        )
    }
}

/// レポートを1行のJSONとして追記する。
pub fn append(dir: &Path, report: &ScanReport) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("scan-log.jsonl");

    // JSONLなので必ず1行に収める。to_string はインデントを入れない
    let line = serde_json::to_string(report)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ScanReport {
        let mut r = ScanReport::new(chrono::Utc::now());
        r.collectors.push(CollectorReport::ok(
            "labels",
            "gh label list",
            5,
            120,
        ));
        r.collectors.push(CollectorReport::failed(
            "routines",
            "GET /v1/code/triggers",
            "認証トークンが設定されていない",
        ));
        r.collectors.push(CollectorReport::skipped(
            "actions",
            ".github/workflows/*.yml",
            "configで無効化されている",
        ));
        r
    }

    #[test]
    fn failures_always_carry_a_reason() {
        for c in sample().problems() {
            assert!(
                c.note.is_some(),
                "{} が理由なしで記録されている",
                c.name
            );
        }
    }

    #[test]
    fn problems_exclude_successful_collectors() {
        let r = sample();
        let names: Vec<_> = r.problems().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["routines", "actions"]);
    }

    #[test]
    fn serializes_to_exactly_one_line() {
        // JSONLが壊れると過去のスキャンごと読めなくなる
        let line = serde_json::to_string(&sample()).unwrap();
        assert!(!line.contains('\n'));
    }

    #[test]
    fn appends_without_losing_previous_scans() {
        let dir = std::env::temp_dir().join(format!(
            "beltmap-scanlog-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        append(&dir, &sample()).unwrap();
        append(&dir, &sample()).unwrap();

        let body = std::fs::read_to_string(dir.join("scan-log.jsonl")).unwrap();
        let lines: Vec<_> = body.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "追記ではなく上書きされている");

        for l in lines {
            serde_json::from_str::<ScanReport>(l).expect("各行が単独で読めること");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn summary_counts_problem_collectors() {
        let s = sample().summary_line();
        assert!(s.contains("問題のあるコレクター 2"), "{s}");
    }
}
