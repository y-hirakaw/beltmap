//! 観測されたラベル遷移(5.1)。
//!
//! `gh api repos/{owner}/{repo}/issues/events` をページングして読む。
//! per-issue の timeline は使わない。issue数だけAPIコールを消費するのに対し、
//! リポジトリ単位なら同じラベルイベントが1系列で取れる。
//!
//! 注意: `actor.login` で機械は特定できない。クラウドルーチンの操作は実行者の
//! GitHubアカウントとして記録されるため、ルーチン同士が区別できない。
//! 遷移の機械への帰属は推測層(5.2)の担当であり、ここで実線にしてはならない。

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct IssueEvent {
    pub event: String,
    pub created_at: String,
    #[serde(default)]
    pub label: Option<Label>,
    #[serde(default)]
    pub actor: Option<Actor>,
    #[serde(default)]
    pub issue: Option<IssueRef>,
}

#[derive(Debug, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Actor {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct IssueRef {
    pub number: u64,
}

/// ラベルの付与/剥奪だけを取り出したもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelTransition {
    pub issue: u64,
    pub label: String,
    pub added: bool,
    pub created_at: String,
    pub actor: Option<String>,
}

pub fn parse(raw: &[u8]) -> Result<Vec<IssueEvent>, serde_json::Error> {
    serde_json::from_slice(raw)
}

/// ラベルイベントのみ抽出する。他のイベント種別(closed, assigned など)は捨てる。
pub fn label_transitions(events: &[IssueEvent]) -> Vec<LabelTransition> {
    events
        .iter()
        .filter_map(|e| {
            let added = match e.event.as_str() {
                "labeled" => true,
                "unlabeled" => false,
                _ => return None,
            };
            Some(LabelTransition {
                issue: e.issue.as_ref()?.number,
                label: e.label.as_ref()?.name.clone(),
                added,
                created_at: e.created_at.clone(),
                actor: e.actor.as_ref().map(|a| a.login.clone()),
            })
        })
        .collect()
}

/// レーンからレーンへの流れ1本。
///
/// **どの機械が起こしたかは含まない。**actorでは機械を特定できないため、
/// 帰属は推測層の担当である。ただし「issueがAからBへ動いた」こと自体は
/// 決定論で確定するので、この辺は `confirmed` として描いてよい。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneFlow {
    pub from: String,
    pub to: String,
    pub issue: u64,
}

/// 同一issue上で「Aが外れ、ほぼ同時にBが付いた」組を流れとみなす。
///
/// 付与と剥奪の記録順は保証されない(実測で、剥奪が先の issue と後の issue が
/// 混在した)。したがって順序ではなく時間の近さで組にする。
pub fn lane_flows(transitions: &[LabelTransition], window_secs: i64) -> Vec<LaneFlow> {
    use std::collections::BTreeMap;

    let mut by_issue: BTreeMap<u64, Vec<&LabelTransition>> = BTreeMap::new();
    for t in transitions {
        by_issue.entry(t.issue).or_default().push(t);
    }

    let mut flows = Vec::new();
    for (issue, ts) in by_issue {
        let removed: Vec<_> = ts.iter().filter(|t| !t.added).collect();
        let added: Vec<_> = ts.iter().filter(|t| t.added).collect();
        let mut used = vec![false; added.len()];

        for r in removed {
            let Some(rt) = parse_time(&r.created_at) else {
                continue;
            };
            // 時間の近い付与を1つだけ相手にする
            let mut best: Option<(usize, i64)> = None;
            for (i, a) in added.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let Some(at) = parse_time(&a.created_at) else {
                    continue;
                };
                let diff = (at - rt).num_seconds().abs();
                if diff <= window_secs && best.is_none_or(|(_, b)| diff < b) {
                    best = Some((i, diff));
                }
            }
            if let Some((i, _)) = best {
                used[i] = true;
                flows.push(LaneFlow {
                    from: r.label.clone(),
                    to: added[i].label.clone(),
                    issue,
                });
            }
        }
    }

    flows
}

fn parse_time(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/issues-events.json");

    #[test]
    fn extracts_only_label_events() {
        let events = parse(FIXTURE).expect("fixture should parse");
        let t = label_transitions(&events);

        // fixture には labeled 以外のイベントも含まれる
        assert!(events.len() > t.len());
        assert!(t.iter().all(|x| !x.label.is_empty()));
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
    fn pairs_removal_and_addition_into_a_flow() {
        let t = vec![
            tr(1, "ai-process:ready", false, "2026-07-26T10:00:00Z"),
            tr(1, "ai-process:spec-draft", true, "2026-07-26T10:00:01Z"),
        ];
        let flows = lane_flows(&t, 60);

        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].from, "ai-process:ready");
        assert_eq!(flows[0].to, "ai-process:spec-draft");
    }

    #[test]
    fn pairs_regardless_of_record_order() {
        // 実測では付与が先に記録されたissueと剥奪が先のissueが混在した
        let t = vec![
            tr(2, "ai-process:spec-draft", true, "2026-07-26T10:00:00Z"),
            tr(2, "ai-process:ready", false, "2026-07-26T10:00:01Z"),
        ];
        let flows = lane_flows(&t, 60);

        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].from, "ai-process:ready");
        assert_eq!(flows[0].to, "ai-process:spec-draft");
    }

    #[test]
    fn lone_addition_is_not_a_flow() {
        // blocked を足しただけ(readyは外していない)。どこからも流れてきていない
        let t = vec![tr(4, "ai-process:blocked", true, "2026-07-26T10:00:00Z")];
        assert!(lane_flows(&t, 60).is_empty());
    }

    #[test]
    fn distant_events_are_not_paired() {
        // 半年後に別のラベルが付いただけのものを流れと呼んではならない
        let t = vec![
            tr(5, "ai-process:ready", false, "2026-01-01T00:00:00Z"),
            tr(5, "ai-process:doc", true, "2026-07-01T00:00:00Z"),
        ];
        assert!(lane_flows(&t, 60).is_empty());
    }

    #[test]
    fn does_not_cross_issues() {
        let t = vec![
            tr(1, "ai-process:ready", false, "2026-07-26T10:00:00Z"),
            tr(2, "ai-process:spec-draft", true, "2026-07-26T10:00:00Z"),
        ];
        assert!(lane_flows(&t, 60).is_empty());
    }

    #[test]
    fn keeps_issue_number_and_actor() {
        let events = parse(FIXTURE).unwrap();
        let t = label_transitions(&events);

        let first = t.first().expect("fixture has label events");
        assert_eq!(first.issue, 24);
        assert!(first.added);
        assert_eq!(first.actor.as_deref(), Some("example-user"));
    }
}
