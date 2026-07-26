//! スキャンの組み立て。コレクターの結果をIRにまとめる。

use std::collections::BTreeSet;

use crate::collectors::transitions::LaneFlow;
use crate::ir::{Confidence, Edge, Lane, LaneRelevance, Unknown};

/// ラベルが工場のレーンかどうかを、根拠つきで判定する。
///
/// 使う証拠は2つで、どちらも決定論:
///   1. 遷移が観測された … 機械が実際に動かしている
///   2. 機械の定義本文にラベル名が literal で出てくる … 文字列一致であって推測ではない
///
/// 証拠が無いものを消さずに `NoEvidence` として残すのは、**消すと「工場に
/// 無いこと」を断定したことになる**ため。判定を間違えたときに画面から
/// 気づけなくなる。
pub fn classify_lanes(lanes: &mut [Lane], flows: &[LaneFlow], definition_texts: &[String]) {
    for lane in lanes.iter_mut() {
        let mut evidence = Vec::new();

        let moved = flows
            .iter()
            .filter(|f| f.from == lane.label || f.to == lane.label)
            .count();
        if moved > 0 {
            evidence.push(format!("{moved}件の遷移を観測"));
        }

        let mentions = definition_texts
            .iter()
            .filter(|t| t.contains(&lane.label))
            .count();
        if mentions > 0 {
            evidence.push(format!("{mentions}件の機械定義に記載"));
        }

        lane.relevance = if evidence.is_empty() {
            LaneRelevance::NoEvidence
        } else {
            LaneRelevance::Factory
        };
        lane.evidence = evidence;
    }
}

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

    let mut out = Vec::new();

    for l in lanes {
        // 工場のレーンだという証拠が無いものは、行き止まりを論じる対象にしない。
        // GitHubの標準ラベルを全部「行き止まり」として並べても意味がない
        if l.relevance != LaneRelevance::Factory {
            continue;
        }
        let has_in = inflow.contains(l.label.as_str());
        let has_out = outflow.contains(l.label.as_str());

        if has_out {
            continue;
        }

        if has_in {
            out.push(Unknown::OrphanLane {
                label: l.label.clone(),
                note: format!("流入は観測されているが流出が無い。在庫{}件", l.count),
            });
        } else if l.count > 0 {
            // issueがそのレーンで生まれると流入は記録されない。
            // 行き止まりの可能性はあるが、OrphanLaneより証拠が弱い
            out.push(Unknown::UnobservedLane {
                label: l.label.clone(),
                note: format!(
                    "在庫{}件だが流入も流出も観測されていない。issueがこのレーンで作られた可能性",
                    l.count
                ),
            });
        }
    }

    out
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
            relevance: LaneRelevance::Factory,
            evidence: Vec::new(),
        }
    }

    #[test]
    fn transitions_are_evidence_of_being_a_lane() {
        let mut lanes = vec![lane("ready", 2), lane("bug", 0)];
        classify_lanes(&mut lanes, &[flow("ready", "doc", 1)], &[]);

        assert_eq!(lanes[0].relevance, LaneRelevance::Factory);
        assert_eq!(lanes[1].relevance, LaneRelevance::NoEvidence);
    }

    #[test]
    fn mention_in_machine_definition_is_evidence() {
        // 遷移が1件も無くても、機械の定義に書かれていれば工場の一部。
        // まだ動いていないだけの建設中のレーンを拾うのに要る
        let mut lanes = vec![lane("ai-process:doc", 1)];
        let defs = vec!["spec-draft を外して ai-process:doc を付ける".to_string()];
        classify_lanes(&mut lanes, &[], &defs);

        assert_eq!(lanes[0].relevance, LaneRelevance::Factory);
        assert!(lanes[0].evidence[0].contains("機械定義"));
    }

    #[test]
    fn lanes_without_evidence_are_kept_not_deleted() {
        // 消すと「工場に無い」と断定したことになる
        let mut lanes = vec![lane("wontfix", 0)];
        classify_lanes(&mut lanes, &[], &[]);
        assert_eq!(lanes.len(), 1);
    }

    #[test]
    fn stock_born_in_place_is_reported_as_weaker_signal() {
        // issueがそのレーンで作られると流入遷移が残らない。
        // 行き止まりかもしれないが OrphanLane と同じ強さで言ってはならない
        let lanes = vec![lane("ai-process:doc", 1)];
        let found = orphan_lanes(&lanes, &[]);

        assert_eq!(found.len(), 1);
        assert!(matches!(found[0], Unknown::UnobservedLane { .. }));
    }

    #[test]
    fn unrelated_labels_are_not_reported_as_dead_ends() {
        // GitHubの標準ラベルを行き止まりとして並べても意味がない
        let mut lanes = vec![lane("bug", 3)];
        classify_lanes(&mut lanes, &[], &[]);
        assert!(orphan_lanes(&lanes, &[]).is_empty());
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
    fn stock_without_inflow_is_not_called_an_orphan() {
        // 在庫があるだけでは行き止まりと言い切れない。まだ誰も処理していない
        // だけかもしれず、決めつけると推測になる。
        // 報告はするが OrphanLane と同じ強さで言ってはならない
        let lanes = vec![lane("ready", 5)];
        let found = orphan_lanes(&lanes, &[]);

        assert!(
            !found
                .iter()
                .any(|u| matches!(u, Unknown::OrphanLane { .. })),
            "流入の観測が無いのに行き止まりと断定している"
        );
    }

    #[test]
    fn lane_with_outflow_is_not_an_orphan() {
        let lanes = vec![lane("ready", 2), lane("doc", 1)];
        let flows = vec![flow("ready", "doc", 1), flow("doc", "done", 2)];
        assert!(orphan_lanes(&lanes, &flows).is_empty());
    }
}
