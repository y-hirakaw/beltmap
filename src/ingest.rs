//! 推測層の回答を取り込む(5.2)。
//!
//! **検証を通ったものだけを反映する。**落ちた項目はその項目だけ捨て、
//! ファイル全体は捨てない。捨てた理由はスキャンログに残す。

use crate::enrich::{EnrichmentRequest, EnrichmentResponse, Verdict, verify};
use crate::ir::{Confidence, Edge, Machine};

pub struct Applied {
    pub accepted: usize,
    /// 弾いた回答と理由。黙って捨てない
    pub rejected: Vec<String>,
}

/// 回答を機械に反映する。
pub fn apply(
    machines: &mut [Machine],
    request: &EnrichmentRequest,
    response: &EnrichmentResponse,
) -> Applied {
    let mut accepted = 0;
    let mut rejected = Vec::new();

    for r in &response.results {
        match verify(r, request) {
            Verdict::Accept => {
                let Some(m) = machines.iter_mut().find(|m| m.id == r.machine_id) else {
                    rejected.push(format!("{}: 対象の機械が見つからない", r.machine_id));
                    continue;
                };

                // 空の回答は「埋まらなかった」という結論であり、成果である。
                // unknown のままにして、次回も同じ穴として出し続ける
                if r.reads.is_empty() && r.writes.is_empty() {
                    if let Some(s) = &r.summary {
                        m.summary = Some(s.clone());
                    }
                    rejected.push(format!(
                        "{}: 根拠にラベルの記述が無く空で返された(未解決のまま)",
                        r.machine_id
                    ));
                    continue;
                }

                m.reads = r.reads.clone();
                m.writes = r.writes.clone();
                m.labels_confidence = Confidence::Inferred;
                if let Some(s) = &r.summary {
                    m.summary = Some(s.clone());
                }
                m.provenance.push("enrichment.json (推測)".to_string());
                accepted += 1;
            }
            Verdict::Stale => rejected.push(format!(
                "{}: 根拠が変わっている。推測は無効",
                r.machine_id
            )),
            Verdict::UnknownLabel(l) => rejected.push(format!(
                "{}: 実在しないラベル `{l}` を含む",
                r.machine_id
            )),
            Verdict::NotRequested => rejected.push(format!(
                "{}: 依頼していない機械への回答",
                r.machine_id
            )),
        }
    }

    Applied { accepted, rejected }
}

/// 推測された読む/書くから機械とレーンの辺を作る。
///
/// **確信度は必ず `inferred`。**観測遷移から作る辺(confirmed)と同じ見た目で
/// 描いてはならない。
pub fn machine_edges(machines: &[Machine]) -> Vec<Edge> {
    let mut edges = Vec::new();

    for m in machines {
        if m.labels_confidence != Confidence::Inferred {
            continue;
        }
        for r in &m.reads {
            edges.push(Edge {
                from: format!("lane:{r}"),
                to: m.id.clone(),
                condition: None,
                confidence: Confidence::Inferred,
                provenance: vec!["定義本文からの推測".to_string()],
            });
        }
        for w in &m.writes {
            edges.push(Edge {
                from: m.id.clone(),
                to: format!("lane:{w}"),
                condition: None,
                confidence: Confidence::Inferred,
                provenance: vec!["定義本文からの推測".to_string()],
            });
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrich::{ENRICH_VERSION, EnrichmentResult, EnrichmentTask, source_hash};
    use crate::ir::{MachineStatus, Runtime, Trigger};

    fn machine(id: &str) -> Machine {
        Machine {
            id: id.into(),
            name: id.into(),
            runtime: Runtime::DesktopTask,
            trigger: Trigger::Unknown,
            reads: vec![],
            writes: vec![],
            status: MachineStatus::Building,
            confidence: Confidence::Confirmed,
            labels_confidence: Confidence::Unknown,
            provenance: vec![],
            summary: None,
            working_dir: None,
        }
    }

    fn request(body: &str) -> EnrichmentRequest {
        EnrichmentRequest {
            version: ENRICH_VERSION,
            tasks: vec![EnrichmentTask {
                machine_id: "m".into(),
                source_text: body.into(),
                source_hash: source_hash(body),
                fields: vec![],
                known_labels: vec!["ready".into(), "done".into()],
            }],
        }
    }

    fn response(reads: Vec<&str>, hash: &str) -> EnrichmentResponse {
        EnrichmentResponse {
            version: ENRICH_VERSION,
            results: vec![EnrichmentResult {
                machine_id: "m".into(),
                source_hash: hash.into(),
                reads: reads.into_iter().map(String::from).collect(),
                writes: vec!["done".into()],
                summary: Some("要約".into()),
                trigger_detail: None,
            }],
        }
    }

    #[test]
    fn applied_labels_are_marked_inferred_not_confirmed() {
        let mut ms = vec![machine("m")];
        let req = request("本文");
        let res = response(vec!["ready"], &source_hash("本文"));

        let applied = apply(&mut ms, &req, &res);

        assert_eq!(applied.accepted, 1);
        assert_eq!(ms[0].labels_confidence, Confidence::Inferred);
        // 機械が存在すること自体は実測のままで、格下げされない
        assert_eq!(ms[0].confidence, Confidence::Confirmed);
    }

    #[test]
    fn stale_answers_do_not_touch_the_machine() {
        let mut ms = vec![machine("m")];
        let req = request("本文");
        let res = response(vec!["ready"], &source_hash("書き換え後"));

        let applied = apply(&mut ms, &req, &res);

        assert_eq!(applied.accepted, 0);
        assert!(ms[0].reads.is_empty());
        assert_eq!(ms[0].labels_confidence, Confidence::Unknown);
        assert!(applied.rejected[0].contains("根拠が変わっている"));
    }

    #[test]
    fn invented_labels_are_rejected_entirely() {
        // 1つでも実在しないラベルがあれば、その回答は丸ごと採らない
        let mut ms = vec![machine("m")];
        let req = request("本文");
        let res = response(vec!["ready", "存在しないラベル"], &source_hash("本文"));

        let applied = apply(&mut ms, &req, &res);

        assert_eq!(applied.accepted, 0);
        assert!(ms[0].reads.is_empty());
    }

    #[test]
    fn empty_answer_stays_unresolved() {
        // 「埋まらなかった」は正しい結果だが、埋まったことにはしない。
        // 次回も同じ穴として出し続ける
        let mut ms = vec![machine("m")];
        let req = request("本文");
        let mut res = response(vec![], &source_hash("本文"));
        res.results[0].writes = vec![];

        let applied = apply(&mut ms, &req, &res);

        assert_eq!(applied.accepted, 0);
        assert_eq!(ms[0].labels_confidence, Confidence::Unknown);
        // 要約だけは根拠から取れているので反映してよい
        assert!(ms[0].summary.is_some());
    }

    #[test]
    fn machine_edges_are_always_inferred() {
        let mut ms = vec![machine("m")];
        apply(&mut ms, &request("本文"), &response(vec!["ready"], &source_hash("本文")));

        let edges = machine_edges(&ms);
        assert!(!edges.is_empty());
        assert!(edges.iter().all(|e| e.confidence == Confidence::Inferred));
    }

    #[test]
    fn unresolved_machines_produce_no_edges() {
        let ms = vec![machine("m")];
        assert!(machine_edges(&ms).is_empty());
    }
}
