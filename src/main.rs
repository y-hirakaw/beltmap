mod collectors;
mod config;
mod enrich;
mod ir;
mod proc;
mod run;
mod scan;
mod scanlog;

use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("scan") => {
            let Some(repo) = args.get(1) else {
                eprintln!("使い方: beltmap scan <owner/repo>");
                eprintln!("  <owner/repo> は issue とラベルがあるリポジトリ(状態リポジトリ)");
                return std::process::ExitCode::from(2);
            };
            do_scan(repo)
        }
        _ => {
            println!("beltmap {}", env!("CARGO_PKG_VERSION"));
            println!();
            println!("  beltmap scan <owner/repo>   工場をスキャンして .beltmap/ に書き出す");
            std::process::ExitCode::SUCCESS
        }
    }
}

fn do_scan(repo: &str) -> std::process::ExitCode {
    let out = PathBuf::from(".beltmap");

    let outcome = match run::scan_all(repo, &out) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("スキャンに失敗した: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    println!("{}", outcome.report.summary_line());

    // 取れなかったものを黙らせない(計画書5.3)
    let problems = outcome.report.problems();
    if !problems.is_empty() {
        println!();
        println!("取得できなかったもの:");
        for c in problems {
            let reason = c.note.as_deref().unwrap_or("(理由なし)");
            println!("  {} … {}", c.name, reason);
        }
    }

    // ローカル機械が居ない場合は別マシンの可能性を明示する(計画書5.3)
    let local = outcome
        .ir
        .machines
        .iter()
        .filter(|m| m.runtime == ir::Runtime::DesktopTask)
        .count();
    if local == 0 {
        println!();
        println!("このマシンにはローカル機械が見つかりません。別マシンで動作している可能性があります。");
    }

    println!();
    println!("  .beltmap/ir.json / .beltmap/scan-log.jsonl に書き出した");

    std::process::ExitCode::SUCCESS
}
