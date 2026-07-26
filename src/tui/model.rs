//! 地図の描画モデル。IRから行の並びを組み立てる。
//!
//! 描画そのものと分けてあるのは、並び順や畳み方の判断をテストしたいため。
//! ratatuiに触れずに検証できる範囲をここに寄せる。

use std::collections::BTreeSet;

use crate::ir::{Ir, Lane, LaneRelevance, Machine, Unknown};

/// レーンの状態。表示の強さを決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneMark {
    /// 流入があるのに流出が無い。行き止まりの強い証拠
    DeadEnd,
    /// 在庫はあるが流入も流出も観測されていない
    NeedsCheck,
    Normal,
}

#[derive(Debug, Clone)]
pub enum Row {
    Header(String),
    Lane {
        label: String,
        count: usize,
        oldest_days: Option<i64>,
        mark: LaneMark,
        /// 木構造の深さ。流れの下流ほど深い
        depth: usize,
    },
    Flow {
        to: String,
        note: String,
        depth: usize,
    },
    Machine {
        id: String,
        name: String,
        trigger: String,
        /// 読む/書くラベルが埋まっていない
        unresolved: bool,
    },
    Note(String),
}

impl Row {
    /// カーソルを合わせられる行か。
    pub fn selectable(&self) -> bool {
        matches!(self, Row::Lane { .. } | Row::Machine { .. })
    }
}

fn mark_of(label: &str, unknowns: &[Unknown]) -> LaneMark {
    for u in unknowns {
        match u {
            Unknown::OrphanLane { label: l, .. } if l == label => return LaneMark::DeadEnd,
            Unknown::UnobservedLane { label: l, .. } if l == label => return LaneMark::NeedsCheck,
            _ => {}
        }
    }
    LaneMark::Normal
}

/// 流れの上流から下流へ並べる。
///
/// 上流(流入が無いレーン)を先頭に置き、そこから辿れるものを下にぶら下げる。
/// 閉路があっても止まらないよう、一度出したレーンは二度出さない。
pub fn build_rows(ir: &Ir) -> Vec<Row> {
    let factory: Vec<&Lane> = ir
        .lanes
        .iter()
        .filter(|l| l.relevance == LaneRelevance::Factory)
        .collect();

    // lane:接頭辞を外して素のラベル名で辺を持つ
    let edges: Vec<(String, String, String)> = ir
        .edges
        .iter()
        .map(|e| {
            (
                e.from.trim_start_matches("lane:").to_string(),
                e.to.trim_start_matches("lane:").to_string(),
                e.provenance.first().cloned().unwrap_or_default(),
            )
        })
        .collect();

    let has_inflow: BTreeSet<&str> = edges.iter().map(|(_, to, _)| to.as_str()).collect();

    let mut rows = Vec::new();
    let mut emitted: BTreeSet<String> = BTreeSet::new();

    rows.push(Row::Header("レーン".to_string()));

    // 上流から
    for lane in factory.iter().filter(|l| !has_inflow.contains(l.label.as_str())) {
        emit_lane(lane, 0, &factory, &edges, ir, &mut rows, &mut emitted);
    }
    // 閉路の中だけに居るなど、上流から辿れなかったもの
    for lane in &factory {
        if !emitted.contains(&lane.label) {
            emit_lane(lane, 0, &factory, &edges, ir, &mut rows, &mut emitted);
        }
    }

    let hidden = ir.lanes.len() - factory.len();
    if hidden > 0 {
        // 畳んだことを黙らない
        rows.push(Row::Note(format!(
            "工場のレーンだという証拠が無いラベル {hidden}件を省略"
        )));
    }

    rows.push(Row::Header(format!("機械 ({})", ir.machines.len())));
    for m in &ir.machines {
        rows.push(machine_row(m));
    }

    rows
}

fn emit_lane(
    lane: &Lane,
    depth: usize,
    factory: &[&Lane],
    edges: &[(String, String, String)],
    ir: &Ir,
    rows: &mut Vec<Row>,
    emitted: &mut BTreeSet<String>,
) {
    if !emitted.insert(lane.label.clone()) {
        return;
    }

    rows.push(Row::Lane {
        label: lane.label.clone(),
        count: lane.count,
        oldest_days: lane.oldest_days,
        mark: mark_of(&lane.label, &ir.unknowns),
        depth,
    });

    for (from, to, note) in edges {
        if from != &lane.label {
            continue;
        }
        rows.push(Row::Flow {
            to: to.clone(),
            note: note.clone(),
            depth: depth + 1,
        });
        if let Some(next) = factory.iter().find(|l| &l.label == to) {
            emit_lane(next, depth + 1, factory, edges, ir, rows, emitted);
        }
    }
}

fn machine_row(m: &Machine) -> Row {
    let trigger = match &m.trigger {
        crate::ir::Trigger::Schedule { detail } => detail.clone(),
        crate::ir::Trigger::Event { detail } => detail.clone(),
        crate::ir::Trigger::Manual => "手動".to_string(),
        crate::ir::Trigger::Unknown => "?".to_string(),
    };
    Row::Machine {
        id: m.id.clone(),
        name: m.name.clone(),
        trigger,
        unresolved: m.reads.is_empty() && m.writes.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Confidence, Edge, IR_VERSION, MachineStatus, Runtime, Trigger};

    fn lane(label: &str, count: usize, relevance: LaneRelevance) -> Lane {
        Lane {
            label: label.into(),
            count,
            oldest_days: None,
            relevance,
            evidence: Vec::new(),
        }
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: format!("lane:{from}"),
            to: format!("lane:{to}"),
            condition: None,
            confidence: Confidence::Confirmed,
            provenance: vec!["1件の遷移を観測".into()],
        }
    }

    fn ir(lanes: Vec<Lane>, edges: Vec<Edge>, unknowns: Vec<Unknown>) -> Ir {
        Ir {
            version: IR_VERSION,
            scanned_at: chrono::Utc::now(),
            scanned_on: "test".into(),
            machines: Vec::new(),
            lanes,
            edges,
            unknowns,
            answers: Vec::new(),
        }
    }

    fn lane_labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .filter_map(|r| match r {
                Row::Lane { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn upstream_lanes_come_first() {
        let m = ir(
            vec![
                lane("spec-draft", 1, LaneRelevance::Factory),
                lane("ready", 2, LaneRelevance::Factory),
            ],
            vec![edge("ready", "spec-draft")],
            vec![],
        );
        assert_eq!(lane_labels(&build_rows(&m)), vec!["ready", "spec-draft"]);
    }

    #[test]
    fn downstream_lanes_are_indented() {
        let m = ir(
            vec![
                lane("ready", 2, LaneRelevance::Factory),
                lane("spec-draft", 1, LaneRelevance::Factory),
            ],
            vec![edge("ready", "spec-draft")],
            vec![],
        );
        let rows = build_rows(&m);
        let depths: Vec<usize> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Lane { depth, .. } => Some(*depth),
                _ => None,
            })
            .collect();
        assert_eq!(depths, vec![0, 1]);
    }

    #[test]
    fn cycles_do_not_loop_forever() {
        // unblocker が blocked → ready に戻すので工場には閉路がある
        let m = ir(
            vec![
                lane("ready", 1, LaneRelevance::Factory),
                lane("blocked", 1, LaneRelevance::Factory),
            ],
            vec![edge("ready", "blocked"), edge("blocked", "ready")],
            vec![],
        );
        let rows = build_rows(&m);
        assert_eq!(lane_labels(&rows).len(), 2, "同じレーンを二度出している");
    }

    #[test]
    fn lanes_without_evidence_are_hidden_but_counted() {
        let m = ir(
            vec![
                lane("ready", 1, LaneRelevance::Factory),
                lane("bug", 0, LaneRelevance::NoEvidence),
            ],
            vec![],
            vec![],
        );
        let rows = build_rows(&m);

        assert_eq!(lane_labels(&rows), vec!["ready"]);
        let note = rows.iter().any(|r| matches!(r, Row::Note(n) if n.contains("1件")));
        assert!(note, "省略した件数が出ていない");
    }

    #[test]
    fn dead_end_and_needs_check_are_distinguished() {
        let m = ir(
            vec![
                lane("spec-draft", 2, LaneRelevance::Factory),
                lane("doc", 1, LaneRelevance::Factory),
            ],
            vec![],
            vec![
                Unknown::OrphanLane {
                    label: "spec-draft".into(),
                    note: String::new(),
                },
                Unknown::UnobservedLane {
                    label: "doc".into(),
                    note: String::new(),
                },
            ],
        );
        let rows = build_rows(&m);
        let marks: Vec<LaneMark> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Lane { mark, .. } => Some(*mark),
                _ => None,
            })
            .collect();
        assert_eq!(marks, vec![LaneMark::DeadEnd, LaneMark::NeedsCheck]);
    }

    #[test]
    fn headers_are_not_selectable() {
        let m = ir(vec![lane("ready", 1, LaneRelevance::Factory)], vec![], vec![]);
        let rows = build_rows(&m);
        assert!(!rows[0].selectable());
        assert!(rows.iter().any(Row::selectable));
    }

    #[test]
    fn machines_appear_with_unresolved_flag() {
        let mut m = ir(vec![], vec![], vec![]);
        m.machines.push(Machine {
            id: "desktop:triage".into(),
            name: "triage".into(),
            runtime: Runtime::DesktopTask,
            trigger: Trigger::Schedule {
                detail: "0 * * * *".into(),
            },
            reads: vec![],
            writes: vec![],
            status: MachineStatus::Building,
            confidence: Confidence::Confirmed,
            provenance: vec![],
            summary: None,
            working_dir: None,
        });
        let rows = build_rows(&m);
        let found = rows.iter().any(|r| matches!(r, Row::Machine { unresolved, .. } if *unresolved));
        assert!(found);
    }
}
