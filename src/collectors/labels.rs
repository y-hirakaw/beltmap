//! レーンと在庫(5.1)。`gh` 経由でラベル一覧とオープンissueを読む。
//!
//! レーン = ラベル1つ。在庫 = そのラベルが付いたオープンissueの数。
//!
//! 滞留日数は**そのレーンに入った時刻**から測る。ラベルが付与された遷移を
//! transitions から引き、最後に付いた時刻を入場時刻とする。
//!
//! 遷移が見つからない場合は issue の作成日時で代用するが、**代用したことを
//! basis に残す。**同じ「5日」でも根拠の強さが違うので、実測と同じ顔で
//! 出してはならない。
//!
//! 代用が要るのは主に、遷移が `issues/events` の取得範囲より古い場合と、
//! transitions コレクター自体が失敗した場合である。**issue作成時に付けた
//! ラベルは代用にならない** — GitHubはそれにも `labeled` イベントを記録する
//! ことを実測で確認した(作成時にラベル付きで作ったissueも observed になった)。

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::collectors::transitions::LabelTransition;
use crate::ir::{Lane, StagnationBasis};

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
pub fn build_lanes(
    labels: &[GhLabel],
    issues: &[GhIssue],
    transitions: &[LabelTransition],
    now: DateTime<Utc>,
) -> Vec<Lane> {
    let mut lanes: Vec<Lane> = labels
        .iter()
        .map(|l| Lane {
            label: l.name.clone(),
            count: 0,
            oldest_days: None,
            oldest_basis: None,
            oldest_issue: None,
            // 分類は scan::classify_lanes が根拠つきで行う
            relevance: crate::ir::LaneRelevance::NoEvidence,
            evidence: Vec::new(),
        })
        .collect();

    for issue in issues {
        for il in &issue.labels {
            let Some(lane) = lanes.iter_mut().find(|l| l.label == il.name) else {
                continue;
            };
            lane.count += 1;

            let (entered, basis) = match entered_at(issue.number, &il.name, transitions) {
                Some(t) => (t, StagnationBasis::Observed),
                None => (issue.created_at, StagnationBasis::IssueCreated),
            };
            let days = (now - entered).num_days();

            if lane.oldest_days.is_none_or(|d| days > d) {
                lane.oldest_days = Some(days);
                lane.oldest_basis = Some(basis);
                lane.oldest_issue = Some(issue.number);
            }
        }
    }

    lanes
}

/// そのissueがそのレーンに入った時刻。
///
/// 同じラベルが何度も付け外しされることがあるので、**最後に付与された時刻**を
/// 採る。最初の付与を採ると、一度出て戻ってきたissueの滞留を過大に見積もる。
fn entered_at(
    issue: u64,
    label: &str,
    transitions: &[LabelTransition],
) -> Option<DateTime<Utc>> {
    transitions
        .iter()
        .filter(|t| t.issue == issue && t.label == label && t.added)
        .filter_map(|t| {
            DateTime::parse_from_rfc3339(&t.created_at)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        })
        .max()
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

    fn tr(issue: u64, label: &str, added: bool, at: &str) -> LabelTransition {
        LabelTransition {
            issue,
            label: label.into(),
            added,
            created_at: at.into(),
            actor: None,
        }
    }

    #[test]
    fn counts_inventory_per_lane() {
        let (labels, issues) = fixture();
        let lanes = build_lanes(&labels, &issues, &[], at("2026-07-26T00:00:00Z"));

        let ready = lanes.iter().find(|l| l.label == "ai-process:ready").unwrap();
        assert_eq!(ready.count, 2);
    }

    #[test]
    fn stagnation_is_measured_from_entering_the_lane() {
        // #1 は7/20作成だが 7/25 に ready が付いた。滞留は1日であって6日ではない
        let (labels, issues) = fixture();
        let t = vec![tr(1, "ai-process:ready", true, "2026-07-25T00:00:00Z")];
        let lanes = build_lanes(&labels, &issues, &t, at("2026-07-26T00:00:00Z"));

        let ready = lanes.iter().find(|l| l.label == "ai-process:ready").unwrap();
        // #2 は遷移が無く作成日時で代用され2日。#1 は観測で1日 → 最古は#2
        assert_eq!(ready.oldest_days, Some(2));
        assert_eq!(ready.oldest_issue, Some(2));
    }

    #[test]
    fn observed_entry_is_marked_as_observed() {
        let (labels, issues) = fixture();
        let t = vec![tr(3, "ai-process:doc", true, "2026-07-25T12:00:00Z")];
        let lanes = build_lanes(&labels, &issues, &t, at("2026-07-26T00:00:00Z"));

        let doc = lanes.iter().find(|l| l.label == "ai-process:doc").unwrap();
        assert_eq!(doc.oldest_basis, Some(StagnationBasis::Observed));
    }

    #[test]
    fn fallback_to_issue_creation_is_marked_as_such() {
        // 代用値を実測と同じ顔で出さない
        let (labels, issues) = fixture();
        let lanes = build_lanes(&labels, &issues, &[], at("2026-07-26T00:00:00Z"));

        let doc = lanes.iter().find(|l| l.label == "ai-process:doc").unwrap();
        assert_eq!(doc.oldest_basis, Some(StagnationBasis::IssueCreated));
    }

    #[test]
    fn re_entering_a_lane_resets_the_clock() {
        // 一度出て戻ってきたissueの滞留を、最初の付与から数えると過大になる
        let (labels, issues) = fixture();
        let t = vec![
            tr(1, "ai-process:ready", true, "2026-07-20T00:00:00Z"),
            tr(1, "ai-process:ready", false, "2026-07-22T00:00:00Z"),
            tr(1, "ai-process:ready", true, "2026-07-25T00:00:00Z"),
        ];
        let lanes = build_lanes(&labels, &issues, &t, at("2026-07-26T00:00:00Z"));

        let ready = lanes.iter().find(|l| l.label == "ai-process:ready").unwrap();
        // #1 は1日。#2(代用2日)のほうが古い
        assert_eq!(ready.oldest_issue, Some(2));
    }

    #[test]
    fn keeps_empty_lanes() {
        // 空のレーンは「未接続」の情報。落とすと建設管理に使えなくなる
        let (labels, issues) = fixture();
        let lanes = build_lanes(&labels, &issues, &[], at("2026-07-26T00:00:00Z"));

        let unused = lanes.iter().find(|l| l.label == "ai-process:unused").unwrap();
        assert_eq!(unused.count, 0);
        assert_eq!(unused.oldest_days, None);
        assert_eq!(unused.oldest_basis, None);
    }
}
