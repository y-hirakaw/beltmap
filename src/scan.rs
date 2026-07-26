//! スキャンの組み立て。コレクターの結果をIRにまとめる。

use std::collections::BTreeSet;

use crate::collectors::transitions::LaneFlow;
use crate::ir::{Confidence, Edge, Lane, Unknown};

/// 観測された流れを辺にする。同じ経路が複数issueで観測されても辺は1本。
pub fn flows_to_edges(flows: &[LaneFlow]) -> Vec<Edge> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut edges = Vec::new();

    for f in flows {
        if !seen.insert((f.from.clone(), f.to.clone())) {
            continue;
        }
        let count = flows.iter().filter(|x| x.from == f.from && x.to == f.to).count();
        edges.push(Edge {
            from: format!("lane:{}", f.from),
            to: format!("lane:{}", f.to),
            condition: None,
            // 「issueがAからBへ動いた」は決定論で確定する。
            // どの機械が動かしたかが不明なだけであり、流れ自体は実測
            confidence: Confidence::Confirmed,
            provenance: vec![format!("issues/events: {count}件の遷移を観測")],
        });
    }

    edges
}

/// 孤児レーンを見つける。
///
/// 流入があるのに流出が観測されていないレーン。工場の行き止まりであり、
/// 「未完成・未接続の可視化」(1章の目的3)の中心になる。
///
/// 在庫があるだけのレーンは孤児と呼ばない。**まだ誰も処理していないだけかも
/// しれず、行き止まりだと決めつけるのは推測になる。**流入の実績があって
/// 流出だけが無いものに限る。
pub fn orphan_lanes(lanes: &[Lane], flows: &[LaneFlow]) -> Vec<Unknown> {
    let inflow: BTreeSet<&str> = flows.iter().map(|f| f.to.as_str()).collect();
    let outflow: BTreeSet<&str> = flows.iter().map(|f| f.from.as_str()).collect();

    lanes
        .iter()
        .filter(|l| inflow.contains(l.label.as_str()) && !outflow.contains(l.label.as_str()))
        .map(|l| Unknown::OrphanLane {
            label: l.label.clone(),
            note: format!(
                "流入は観測されているが流出が無い。在庫{}件",
                l.count
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(from: &str, to: &str, issue: u64) -> LaneFlow {
        LaneFlow {
            from: from.into(),
            to: to.into(),
            issue,
        }
    }

    fn lane(label: &str, count: usize) -> Lane {
        Lane {
            label: label.into(),
            count,
            oldest_days: None,
        }
    }

    #[test]
    fn dedupes_edges_across_issues() {
        let flows = vec![
            flow("ready", "spec-draft", 1),
            flow("ready", "spec-draft", 2),
            flow("ready", "needs-human", 3),
        ];
        let edges = flows_to_edges(&flows);

        assert_eq!(edges.len(), 2);
        assert!(edges[0].provenance[0].contains("2件"));
    }

    #[test]
    fn observed_flow_is_confirmed_not_inferred() {
        let edges = flows_to_edges(&[flow("ready", "spec-draft", 1)]);
        assert_eq!(edges[0].confidence, Confidence::Confirmed);
    }

    #[test]
    fn detects_lane_with_inflow_but_no_outflow() {
        let lanes = vec![lane("ready", 2), lane("doc", 1)];
        let flows = vec![flow("ready", "doc", 1)];

        let orphans = orphan_lanes(&lanes, &flows);
        assert_eq!(orphans.len(), 1);
        match &orphans[0] {
            Unknown::OrphanLane { label, .. } => assert_eq!(label, "doc"),
            other => panic!("想定外: {other:?}"),
        }
    }

    #[test]
    fn stock_without_inflow_is_not_an_orphan() {
        // 在庫があるだけでは行き止まりと言えない。まだ誰も処理していない
        // だけかもしれず、決めつけると推測になる
        let lanes = vec![lane("ready", 5)];
        assert!(orphan_lanes(&lanes, &[]).is_empty());
    }

    #[test]
    fn lane_with_outflow_is_not_an_orphan() {
        let lanes = vec![lane("ready", 2), lane("doc", 1)];
        let flows = vec![flow("ready", "doc", 1), flow("doc", "done", 2)];
        assert!(orphan_lanes(&lanes, &flows).is_empty());
    }
}
