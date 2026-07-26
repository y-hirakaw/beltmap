//! スキャンの実行。コレクターを順に呼び、IRとスキャンログを書く。
//!
//! ここは副作用の層であり、判断ロジックは持たない。各コレクターの
//! パース結果を `scan` に渡して組み立てる。

use std::path::Path;
use std::time::Instant;

use crate::collectors::{desktop_tasks, labels, transitions};
use crate::ir::{Ir, Machine, IR_VERSION};
use crate::proc;
use crate::scan;
use crate::scanlog::{CollectorReport, Gap, ScanReport};

pub struct Outcome {
    pub ir: Ir,
    pub report: ScanReport,
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

    // --- labels ---
    if proc::exists("gh") {
        let t = Instant::now();
        let src = format!("gh label list -R {state_repo}");
        match collect_lanes(state_repo) {
            Ok(l) => {
                report.collectors.push(CollectorReport::ok(
                    "labels",
                    &src,
                    l.len(),
                    t.elapsed().as_millis() as u64,
                ));
                lanes = l;
            }
            Err(e) => report
                .collectors
                .push(CollectorReport::failed("labels", &src, &e)),
        }
    } else {
        report.collectors.push(CollectorReport::skipped(
            "labels",
            "gh label list",
            "`gh` が見つからない。https://cli.github.com/ から導入する",
        ));
    }

    // --- transitions ---
    if proc::exists("gh") {
        let t = Instant::now();
        let src = format!("gh api repos/{state_repo}/issues/events");
        match collect_flows(state_repo) {
            Ok((count, f)) => {
                report.collectors.push(CollectorReport::ok(
                    "transitions",
                    &src,
                    count,
                    t.elapsed().as_millis() as u64,
                ));
                report.transitions = count;
                flows = f;
            }
            Err(e) => report
                .collectors
                .push(CollectorReport::failed("transitions", &src, &e)),
        }
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
                        if let Some(d) = &doc {
                            match desktop_tasks::referenced_skill_path(&d.body) {
                                Some(referenced) => {
                                    report.gaps.push(Gap {
                                        machine_id: format!("desktop:{}", task.id),
                                        fields: vec!["reads".into(), "writes".into()],
                                        reason: format!(
                                            "登録されたSKILL.mdはラッパー。実体は {referenced}"
                                        ),
                                    });
                                    // 参照先の本文がレーン判定の根拠になる。
                                    // ラッパー本文にはラベルが出てこない
                                    if let Ok(body) = std::fs::read_to_string(&referenced) {
                                        definition_texts.push(body);
                                    }
                                }
                                None => definition_texts.push(d.body.clone()),
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

    // --- routines (未実装) ---
    report.collectors.push(CollectorReport::skipped(
        "routines",
        "GET /v1/code/triggers",
        "認証方式が未決のため未実装(計画書5.3)。クラウドルーチンは地図に出ない",
    ));

    // --- 組み立て ---
    let edges = scan::flows_to_edges(&flows);
    scan::classify_lanes(&mut lanes, &flows, &definition_texts);
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
    crate::scanlog::append(out_dir, &report)?;

    Ok(Outcome { ir, report })
}

fn collect_lanes(repo: &str) -> Result<Vec<crate::ir::Lane>, String> {
    let raw = proc::run(
        "gh",
        &["label", "list", "-R", repo, "--limit", "200", "--json", "name,description"],
    )
    .map_err(|e| e.to_string())?;
    let labels = labels::parse_labels(&raw).map_err(|e| e.to_string())?;

    let raw = proc::run(
        "gh",
        &[
            "issue", "list", "-R", repo, "--state", "open", "--limit", "500", "--json",
            "number,createdAt,labels",
        ],
    )
    .map_err(|e| e.to_string())?;
    let issues = labels::parse_issues(&raw).map_err(|e| e.to_string())?;

    Ok(labels::build_lanes(&labels, &issues, chrono::Utc::now()))
}

fn collect_flows(repo: &str) -> Result<(usize, Vec<transitions::LaneFlow>), String> {
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
    let t = transitions::label_transitions(&events);
    // 付与と剥奪が別秒に記録されることがあるため窓は広めに取る
    let flows = transitions::lane_flows(&t, 60);
    Ok((t.len(), flows))
}
