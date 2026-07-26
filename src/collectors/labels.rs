//! レーンと在庫(5.1)。`gh` 経由でラベル一覧とオープンissueを読む。
//!
//! レーン = ラベル1つ。在庫 = そのラベルが付いたオープンissueの数。
//! 滞留日数は issue の作成日時から出す(ラベルが付いた日時ではない点に注意。
//! 正確な滞留は transitions と突き合わせないと出せないので、ここでの値は
//! 「issueが生まれてから何日経っているか」であり、Phase 2で精密化する)。

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::ir::Lane;

#[derive(Debug, Deserialize)]
pub struct GhLabel {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct GhIssue {
    pub number: u64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub labels: Vec<GhIssueLabel>,
}

#[derive(Debug, Deserialize)]
pub struct GhIssueLabel {
    pub name: String,
}

pub fn parse_labels(raw: &[u8]) -> Result<Vec<GhLabel>, serde_json::Error> {
    serde_json::from_slice(raw)
}

pub fn parse_issues(raw: &[u8]) -> Result<Vec<GhIssue>, serde_json::Error> {
    serde_json::from_slice(raw)
}

/// ラベルとissueからレーンを組み立てる。
///
/// 在庫0のラベルも落とさない。**空のレーンは「使われていない」という情報**であり、
/// 建設中の工場では未接続の検出に効く(1章の目的3)。
pub fn build_lanes(labels: &[GhLabel], issues: &[GhIssue], now: DateTime<Utc>) -> Vec<Lane> {
    let mut lanes: Vec<Lane> = labels
        .iter()
        .map(|l| Lane {
            label: l.name.clone(),
            count: 0,
            oldest_days: None,
        })
        .collect();

    for issue in issues {
        for il in &issue.labels {
            let Some(lane) = lanes.iter_mut().find(|l| l.label == il.name) else {
                continue;
            };
            lane.count += 1;
            let days = (now - issue.created_at).num_days();
            lane.oldest_days = Some(lane.oldest_days.map_or(days, |d: i64| d.max(days)));
        }
    }

    lanes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn fixture() -> (Vec<GhLabel>, Vec<GhIssue>) {
        let labels = vec![
            GhLabel {
                name: "ai-process:ready".into(),
                description: String::new(),
            },
            GhLabel {
                name: "ai-process:doc".into(),
                description: String::new(),
            },
            GhLabel {
                name: "ai-process:unused".into(),
                description: String::new(),
            },
        ];
        let issues = vec![
            GhIssue {
                number: 1,
                created_at: at("2026-07-20T00:00:00Z"),
                labels: vec![GhIssueLabel {
                    name: "ai-process:ready".into(),
                }],
            },
            GhIssue {
                number: 2,
                created_at: at("2026-07-24T00:00:00Z"),
                labels: vec![GhIssueLabel {
                    name: "ai-process:ready".into(),
                }],
            },
            GhIssue {
                number: 3,
                created_at: at("2026-07-25T00:00:00Z"),
                labels: vec![GhIssueLabel {
                    name: "ai-process:doc".into(),
                }],
            },
        ];
        (labels, issues)
    }

    #[test]
    fn counts_inventory_per_lane() {
        let (labels, issues) = fixture();
        let lanes = build_lanes(&labels, &issues, at("2026-07-26T00:00:00Z"));

        let ready = lanes.iter().find(|l| l.label == "ai-process:ready").unwrap();
        assert_eq!(ready.count, 2);
    }

    #[test]
    fn oldest_days_takes_the_longest_waiting_issue() {
        let (labels, issues) = fixture();
        let lanes = build_lanes(&labels, &issues, at("2026-07-26T00:00:00Z"));

        let ready = lanes.iter().find(|l| l.label == "ai-process:ready").unwrap();
        // #1 が6日、#2 が2日 → 6日
        assert_eq!(ready.oldest_days, Some(6));
    }

    #[test]
    fn keeps_empty_lanes() {
        // 空のレーンは「未接続」の情報。落とすと建設管理に使えなくなる
        let (labels, issues) = fixture();
        let lanes = build_lanes(&labels, &issues, at("2026-07-26T00:00:00Z"));

        let unused = lanes.iter().find(|l| l.label == "ai-process:unused").unwrap();
        assert_eq!(unused.count, 0);
        assert_eq!(unused.oldest_days, None);
    }
}
