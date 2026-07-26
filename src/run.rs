//! スキャンの実行。コレクターを順に呼び、IRとスキャンログを書く。
//!
//! ここは副作用の層であり、判断ロジックは持たない。各コレクターの
//! パース結果を `scan` に渡して組み立てる。

use std::path::Path;
use std::time::Instant;

use crate::collectors::{desktop_tasks, labels, routines, transitions};
use crate::enrich::{self, EnrichmentRequest, EnrichmentTask};
use crate::ir::{Ir, Machine, IR_VERSION};
use crate::proc;
use crate::ingest;
use crate::scan;
use crate::scanlog::{CollectorReport, Gap, ScanReport};

pub struct Outcome {
    pub ir: Ir,
    pub report: ScanReport,
    /// 推測層に投げる未解決タスクの件数
    pub pending: usize,
}

pub fn scan_all(state_repo: &str, out_dir: &Path) -> std::io::Result<Outcome> {
    let started = chrono::Utc::now();
    let t0 = Instant::now();
    let mut report = ScanReport::new(started);

    let mut machines: Vec<Machine> = Vec::new();
    let mut lanes = Vec::new();
    let mut flows = Vec::new();
    // レーン判定の根拠に使う機械の定義本文
    let mut definition_texts: Vec<String> = Vec::new();
    // 推測層に渡す根拠。(機械id, 根拠の場所, 本文)
    let mut sources: Vec<(String, String, String)> = Vec::new();

    // --- gh を使うコレクター ---
    //
    // 実測でスキャン時間のほぼ全部が gh の待ち時間だった(labels 1012ms /
    // transitions 846ms に対し desktop-tasks 4ms)。互いに独立なので
    // 同時に走らせる。非同期ランタイムは要らない。待っているのは
    // サブプロセスであって、スレッドを2本立てれば足りる
    //
    // 滞留日数の算出にラベルと遷移の両方が要るが、依存するのは*計算*だけで
    // *取得*は互いに独立している。取得を同時に走らせ、合流してから組み立てる
    if proc::exists("gh") {
        let t = Instant::now();
        let (label_res, flow_res) = std::thread::scope(|s| {
            let a = s.spawn(|| fetch_label_data(state_repo));
            let b = s.spawn(|| fetch_transitions(state_repo));
            (a.join(), b.join())
        });
        let elapsed = t.elapsed().as_millis() as u64;

        let src = format!("gh api repos/{state_repo}/issues/events");
        let mut observed: Vec<transitions::LabelTransition> = Vec::new();
        match flow_res {
            Ok(Ok(t)) => {
                report
                    .collectors
                    .push(CollectorReport::ok("transitions", &src, t.len(), elapsed));
                report.transitions = t.len();
                // 付与と剥奪が別秒に記録されることがあるため窓は広めに取る
                flows = transitions::lane_flows(&t, 60);
                observed = t;
            }
            Ok(Err(e)) => report
                .collectors
                .push(CollectorReport::failed("transitions", &src, &e)),
            Err(_) => report.collectors.push(CollectorReport::failed(
                "transitions",
                &src,
                "収集スレッドが異常終了した",
            )),
        }

        let src = format!("gh label list -R {state_repo}");
        match label_res {
            Ok(Ok((labs, issues))) => {
                lanes = labels::build_lanes(&labs, &issues, &observed, chrono::Utc::now());
                report
                    .collectors
                    .push(CollectorReport::ok("labels", &src, lanes.len(), elapsed));
            }
            Ok(Err(e)) => report
                .collectors
                .push(CollectorReport::failed("labels", &src, &e)),
            Err(_) => report.collectors.push(CollectorReport::failed(
                "labels",
                &src,
                "収集スレッドが異常終了した",
            )),
        }
    } else {
        report.collectors.push(CollectorReport::skipped(
            "labels",
            "gh label list",
            "`gh` が見つからない。https://cli.github.com/ から導入する",
        ));
        report.collectors.push(CollectorReport::skipped(
            "transitions",
            "gh api issues/events",
            "`gh` が見つからない。観測遷移が無いため辺は描けない",
        ));
    }

    // --- desktop-tasks ---
    {
        let t = Instant::now();
        match desktop_tasks::default_support_dir() {
            Some(dir) => {
                let files = desktop_tasks::find_registry_files(&dir);
                let src = format!("{} 個のレジストリ", files.len());
                let mut count = 0;
                for f in &files {
                    let Ok(raw) = std::fs::read(f) else { continue };
                    let Ok(parsed) = desktop_tasks::parse(&raw) else {
                        report.collectors.push(CollectorReport::failed(
                            "desktop-tasks",
                            &f.display().to_string(),
                            "JSONの形が想定と違う",
                        ));
                        continue;
                    };
                    for task in &parsed.scheduled_tasks {
                        let doc = std::fs::read_to_string(&task.file_path)
                            .ok()
                            .map(|s| desktop_tasks::parse_skill_md(&s));
                        machines.push(desktop_tasks::to_machine(task, doc.as_ref()));
                        count += 1;

                        // 仕様がラッパー越しなら、根拠は参照先にある
                        let machine_id = format!("desktop:{}", task.id);
                        if let Some(d) = &doc {
                            match desktop_tasks::referenced_skill_path(&d.body) {
                                Some(referenced) => {
                                    report.gaps.push(Gap {
                                        machine_id: machine_id.clone(),
                                        fields: vec!["reads".into(), "writes".into()],
                                        reason: format!(
                                            "登録されたSKILL.mdはラッパー。実体は {referenced}"
                                        ),
                                    });
                                    // 参照先の本文がレーン判定と推測の根拠になる。
                                    // ラッパー本文にはラベルが出てこない
                                    match std::fs::read_to_string(&referenced) {
                                        Ok(body) => {
                                            definition_texts.push(body.clone());
                                            sources.push((machine_id, referenced, body));
                                        }
                                        Err(_) => {
                                            // 参照先が読めない。ラッパー本文を
                                            // 代わりに渡してはならない(材料が無い)
                                        }
                                    }
                                }
                                None => {
                                    definition_texts.push(d.body.clone());
                                    sources.push((
                                        machine_id,
                                        task.file_path.clone(),
                                        d.body.clone(),
                                    ));
                                }
                            }
                        }
                    }
                }
                report.collectors.push(CollectorReport::ok(
                    "desktop-tasks",
                    &src,
                    count,
                    t.elapsed().as_millis() as u64,
                ));
            }
            None => report.collectors.push(CollectorReport::skipped(
                "desktop-tasks",
                "Application Support/Claude",
                "HOME が取得できない",
            )),
        }
    }

    // --- routines ---
    //
    // beltmapは認証情報を持たない。取得は beltmap-routines skill が代行し、
    // ここではその置いたファイルを読むだけ。skillは判断をしないため、
    // 中身はAPIの応答そのものであり実測として扱ってよい(計画書5.3)
    {
        let t = Instant::now();
        let path = out_dir.join("routines.json");
        if path.is_file() {
            let src = path.display().to_string();
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|raw| routines::parse_file(&raw).map_err(|e| e.to_string()))
            {
                Ok(f) => {
                    let age = (chrono::Utc::now() - f.fetched_at).num_days();
                    for t in &f.response.data {
                        machines.push(routines::to_machine(t));
                        // ルーチンのプロンプトも機械の仕様書であり推測の材料になる
                        if let Some(p) = t.prompt() {
                            definition_texts.push(p.to_string());
                            sources.push((
                                t.id.clone(),
                                format!("cloud-routine:{}", t.id),
                                p.to_string(),
                            ));
                        }
                    }
                    let mut rep = CollectorReport::ok(
                        "routines",
                        &src,
                        f.response.data.len(),
                        t.elapsed().as_millis() as u64,
                    );
                    // 取得はユーザー起動なので古くなりうる。黙らない
                    if age >= 1 {
                        rep.note = Some(format!(
                            "{age}日前に取得した情報。beltmap-routines skill で更新できる"
                        ));
                    }
                    report.collectors.push(rep);
                }
                Err(e) => report
                    .collectors
                    .push(CollectorReport::failed("routines", &src, &e)),
            }
        } else {
            report.collectors.push(CollectorReport::skipped(
                "routines",
                "routines.json",
                "未取得。`beltmap-routines` skill を実行するとクラウドルーチンが地図に出る",
            ));
        }
    }

    // --- レーンの分類 ---
    // 推測層への依頼に「実在するラベル一覧」を同梱するため、
    // 依頼を作る前に済ませておく必要がある
    scan::classify_lanes(&mut lanes, &flows, &definition_texts);

    // --- 推測層の回答を取り込む ---
    // 依頼は決定論の結果から作るので、取り込みより先に組み立てる
    let request = build_request_from(&machines, &lanes, &sources, &flows, &definition_texts);
    let mut inferred_edges = Vec::new();
    let enrichment_path = out_dir.join("enrichment.json");
    if enrichment_path.is_file() {
        let t = Instant::now();
        let src = enrichment_path.display().to_string();
        match std::fs::read(&enrichment_path)
            .map_err(|e| e.to_string())
            .and_then(|raw| serde_json::from_slice(&raw).map_err(|e| e.to_string()))
        {
            Ok(res) => {
                let applied = ingest::apply(&mut machines, &request, &res);
                inferred_edges = ingest::machine_edges(&machines);
                report.collectors.push(CollectorReport::ok(
                    "enrichment",
                    &src,
                    applied.accepted,
                    t.elapsed().as_millis() as u64,
                ));
                // 弾いた回答は黙って捨てない
                for r in applied.rejected {
                    report.rejected.push(r);
                }
            }
            Err(e) => report
                .collectors
                .push(CollectorReport::failed("enrichment", &src, &e)),
        }
    } else {
        report.collectors.push(CollectorReport::skipped(
            "enrichment",
            "enrichment.json",
            "推測の回答が無い。`beltmap-enrich` skill を実行すると点線が埋まる",
        ));
    }

    // --- 組み立て ---
    let mut edges = scan::flows_to_edges(&flows);
    edges.extend(inferred_edges);
    let unknowns = scan::orphan_lanes(&lanes, &flows);

    // 決定論で reads/writes が埋まらなかった機械を穴として記録する
    for m in &machines {
        if m.reads.is_empty()
            && m.writes.is_empty()
            && !report.gaps.iter().any(|g| g.machine_id == m.id)
        {
            report.gaps.push(Gap {
                machine_id: m.id.clone(),
                fields: vec!["reads".into(), "writes".into()],
                reason: "定義から読む/書くラベルを決定論で特定できない".into(),
            });
        }
    }

    report.machines = machines.len();
    report.lanes = lanes.len();
    report.unknowns = unknowns.iter().map(|u| format!("{u:?}")).collect();
    report.duration_ms = t0.elapsed().as_millis() as u64;

    let ir = Ir {
        version: IR_VERSION,
        scanned_at: started,
        scanned_on: report.scanned_on.clone(),
        machines,
        lanes,
        edges,
        unknowns,
        answers: Vec::new(),
    };

    std::fs::create_dir_all(out_dir)?;
    std::fs::write(
        out_dir.join("ir.json"),
        serde_json::to_vec_pretty(&ir).map_err(std::io::Error::other)?,
    )?;

    let request = build_request_from(&ir.machines, &ir.lanes, &sources, &flows, &definition_texts);
    std::fs::write(
        out_dir.join("enrichment-request.json"),
        serde_json::to_vec_pretty(&request).map_err(std::io::Error::other)?,
    )?;

    crate::scanlog::append(out_dir, &report)?;

    Ok(Outcome {
        ir,
        report,
        pending: request.tasks.len(),
    })
}

/// 埋まらなかった穴を、根拠つきのタスクとして書き出す。
///
/// 実在するラベル一覧を同梱するのが要点。存在しないラベルを創作されると
/// 幻覚コンベアになるため、**候補を先に絞って渡す。**
fn build_request_from(
    machines: &[Machine],
    lanes: &[crate::ir::Lane],
    sources: &[(String, String, String)],
    _flows: &[transitions::LaneFlow],
    _definition_texts: &[String],
) -> EnrichmentRequest {
    let known_labels: Vec<String> = lanes
        .iter()
        .filter(|l| l.relevance == crate::ir::LaneRelevance::Factory)
        .map(|l| l.label.clone())
        .collect();

    let tasks = machines
        .iter()
        .filter(|m| m.reads.is_empty() && m.writes.is_empty())
        .filter_map(|m| {
            // 根拠が無い機械は依頼しない。材料無しに推測させると嘘が返る
            let (_, _, body) = sources.iter().find(|(id, _, _)| id == &m.id)?;
            Some(EnrichmentTask {
                machine_id: m.id.clone(),
                source_hash: enrich::source_hash(body),
                source_text: body.clone(),
                fields: vec!["reads".into(), "writes".into(), "summary".into()],
                known_labels: known_labels.clone(),
            })
        })
        .collect();

    EnrichmentRequest {
        version: enrich::ENRICH_VERSION,
        tasks,
    }
}

fn fetch_label_data(repo: &str) -> Result<(Vec<labels::GhLabel>, Vec<labels::GhIssue>), String> {
    let raw = proc::run(
        "gh",
        &["label", "list", "-R", repo, "--limit", "200", "--json", "name,description"],
    )
    .map_err(|e| e.to_string())?;
    let labs = labels::parse_labels(&raw).map_err(|e| e.to_string())?;

    let raw = proc::run(
        "gh",
        &[
            "issue", "list", "-R", repo, "--state", "open", "--limit", "500", "--json",
            "number,createdAt,labels",
        ],
    )
    .map_err(|e| e.to_string())?;
    let issues = labels::parse_issues(&raw).map_err(|e| e.to_string())?;

    Ok((labs, issues))
}

fn fetch_transitions(repo: &str) -> Result<Vec<transitions::LabelTransition>, String> {
    let raw = proc::run(
        "gh",
        &[
            "api",
            &format!("repos/{repo}/issues/events?per_page=100"),
            "--paginate",
        ],
    )
    .map_err(|e| e.to_string())?;

    let events = transitions::parse(&raw).map_err(|e| e.to_string())?;
    Ok(transitions::label_transitions(&events))
}
