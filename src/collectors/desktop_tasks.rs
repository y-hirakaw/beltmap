//! Desktopローカルタスク(5.3)。
//!
//! 2ファイルの結合で取る:
//!   - `<root>/*/*/scheduled-tasks.json` … cron / 有効状態 / モデル / cwd
//!   - `filePath` が指す `SKILL.md` … name / description / プロンプト本文
//!
//! パスのUUID2つはハードコードせずグロブで解決する。account単位・
//! インストール単位に分かれるため複数マッチしうる。
//!
//! **レジストリは1つではない。**同じファイル名・同じスキーマのものが
//! 実行系統ごとに別ディレクトリにある(`registry_roots` 参照)。片方だけ
//! 見ると機械を取りこぼす。フィールドの充足度も系統によって違うため、
//! 欠けているものはOptionで受けて`unknown`に落とす。

use serde::Deserialize;

use crate::ir::{Confidence, Machine, MachineStatus, Runtime, Trigger};

/// `~/Library/Application Support/Claude/` からの相対で、
/// `scheduled-tasks.json` を含みうるディレクトリ。
///
/// どちらも `<root>/<account_uuid>/<install_uuid>/scheduled-tasks.json` の形。
pub const REGISTRY_ROOTS: &[&str] = &[
    // Claude Code Desktop のローカルタスク
    "claude-code-sessions",
    // ローカルエージェントモードのタスク
    "local-agent-mode-sessions",
];

#[derive(Debug, Deserialize)]
pub struct ScheduledTasksFile {
    #[serde(rename = "scheduledTasks", default)]
    pub scheduled_tasks: Vec<ScheduledTask>,
}

#[derive(Debug, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    #[serde(rename = "cronExpression")]
    pub cron_expression: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "filePath")]
    pub file_path: String,
    #[serde(default)]
    pub model: Option<String>,
    /// 作業フォルダ。どの工場に属するかの手がかりになるが、
    /// **系統によっては入らない**(local-agent-mode では null)
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(rename = "useWorktree", default)]
    pub use_worktree: bool,
    /// ユーザーがアクセスを許可したフォルダ。`cwd` が無い系統での
    /// 工場所属の手がかりになりうる
    #[serde(rename = "userSelectedFolders", default)]
    pub user_selected_folders: Vec<String>,
}

impl ScheduledTask {
    /// この機械がどの工場に属するかの決定論的な手がかり。
    ///
    /// `cwd` が入っていればそれ。無い系統では許可フォルダで代替する。
    /// どちらも無ければ `None` を返す。ここで推測してはならない。
    pub fn factory_hint(&self) -> Option<&str> {
        self.cwd
            .as_deref()
            .or_else(|| self.user_selected_folders.first().map(|s| s.as_str()))
    }
}

pub fn parse(raw: &[u8]) -> Result<ScheduledTasksFile, serde_json::Error> {
    serde_json::from_slice(raw)
}

/// 全レジストリの `scheduled-tasks.json` を探す。
///
/// `<support>/<root>/<account_uuid>/<install_uuid>/scheduled-tasks.json`。
/// UUID部分はグロブ相当の総当たりで解決する(depth 2固定なので専用の
/// globクレートは要らない)。
pub fn find_registry_files(support_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();

    for root in REGISTRY_ROOTS {
        let base = support_dir.join(root);
        let Ok(accounts) = std::fs::read_dir(&base) else {
            continue;
        };
        for account in accounts.flatten() {
            let Ok(installs) = std::fs::read_dir(account.path()) else {
                continue;
            };
            for install in installs.flatten() {
                let f = install.path().join("scheduled-tasks.json");
                if f.is_file() {
                    found.push(f);
                }
            }
        }
    }

    found.sort();
    found
}

/// macOSでのClaudeのサポートディレクトリ。
pub fn default_support_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::PathBuf::from(home)
            .join("Library/Application Support/Claude"),
    )
}

/// SKILL.md の frontmatter と本文。
pub struct SkillDoc {
    pub name: Option<String>,
    pub description: Option<String>,
    pub body: String,
}

/// frontmatter を読む。
///
/// YAMLパーサは入れない。ここで必要なのは `---` に挟まれた `key: value` の
/// 2キーだけであり、汎用YAMLを解釈する必要がない。
pub fn parse_skill_md(text: &str) -> SkillDoc {
    let mut name = None;
    let mut description = None;

    let rest = match text.strip_prefix("---") {
        Some(after) => match after.split_once("\n---") {
            Some((front, body)) => {
                for line in front.lines() {
                    let Some((k, v)) = line.split_once(':') else {
                        continue;
                    };
                    let v = v.trim().trim_matches('"').to_string();
                    match k.trim() {
                        "name" => name = Some(v),
                        "description" => description = Some(v),
                        _ => {}
                    }
                }
                body.trim_start_matches('\n')
            }
            None => text,
        },
        None => text,
    };

    SkillDoc {
        name,
        description,
        body: rest.trim().to_string(),
    }
}

pub fn to_machine(task: &ScheduledTask, doc: Option<&SkillDoc>) -> Machine {
    let name = doc
        .and_then(|d| d.name.clone())
        .unwrap_or_else(|| task.id.clone());

    let trigger = match &task.cron_expression {
        Some(cron) => Trigger::Schedule {
            detail: cron.clone(),
        },
        None => Trigger::Manual,
    };

    Machine {
        id: format!("desktop:{}", task.id),
        name,
        runtime: Runtime::DesktopTask,
        trigger,
        reads: Vec::new(),
        writes: Vec::new(),
        status: if task.enabled {
            MachineStatus::Building
        } else {
            MachineStatus::Planned
        },
        confidence: Confidence::Confirmed,
        // ラベルは決定論では埋まらない。推測層が埋めるまで unknown
        labels_confidence: Confidence::Unknown,
        provenance: vec![task.file_path.clone()],
        summary: doc.and_then(|d| d.description.clone()),
        working_dir: task.factory_hint().map(str::to_string),
    }
}

/// 登録された `SKILL.md` が、別の場所にある本体の仕様書を指すだけの
/// ラッパーであることがある。その場合、機械の仕様はラッパー本文ではなく
/// 参照先にある。
///
/// 参照先を推測層に渡さないと、ラベルが1つも書かれていないラッパー本文を
/// 読ませることになり、推測は必ず失敗する。**参照は決定論的に辿れる**
/// (本文に絶対パスが書かれている)ので、AIに渡す前にここで解決する。
pub fn referenced_skill_path(body: &str) -> Option<String> {
    // 本文中の絶対パスのうち SKILL.md を指すものを拾う
    let start = body.find('/')?;
    let candidate: String = body[start..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();

    if candidate.ends_with("SKILL.md") {
        return Some(candidate);
    }

    // 「<フォルダ> 内の <相対パス>」形式。フォルダと相対パスが離れて書かれる
    let dir: String = body[start..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    let rel = body.find(".claude/skills/")?;
    let rel_path: String = body[rel..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    if rel_path.ends_with("SKILL.md") {
        Some(format!("{}/{}", dir.trim_end_matches('/'), rel_path))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASKS: &[u8] = include_bytes!("../../tests/fixtures/scheduled-tasks.json");
    const SKILL: &str = include_str!("../../tests/fixtures/desktop-task-SKILL.md");

    #[test]
    fn parses_real_metadata_shape() {
        let f = parse(TASKS).expect("fixture should parse");
        let t = &f.scheduled_tasks[0];

        assert_eq!(t.id, "1");
        assert_eq!(t.cron_expression.as_deref(), Some("0 9 * * *"));
        assert!(t.enabled);
        assert_eq!(t.model.as_deref(), Some("claude-haiku-4-5-20251001"));
        assert_eq!(t.cwd.as_deref(), Some("/Users/example/git"));
        assert!(!t.use_worktree);
    }

    #[test]
    fn reads_frontmatter_and_body() {
        let doc = parse_skill_md(SKILL);
        assert_eq!(doc.name.as_deref(), Some("1"));
        assert_eq!(doc.description.as_deref(), Some("ローカル実行検証用"));
        assert!(doc.body.starts_with("何件のリポジトリが"));
    }

    #[test]
    fn survives_missing_frontmatter() {
        let doc = parse_skill_md("frontmatterのない本文だけのファイル");
        assert!(doc.name.is_none());
        assert_eq!(doc.body, "frontmatterのない本文だけのファイル");
    }

    const LOCAL_AGENT: &[u8] =
        include_bytes!("../../tests/fixtures/scheduled-tasks-local-agent-mode.json");
    const WRAPPER: &str = include_str!("../../tests/fixtures/wrapper-SKILL.md");

    #[test]
    fn parses_the_other_registry_variant() {
        // 同じファイル名・同じスキーマだが別ディレクトリにあり、
        // cwd / model / useWorktree を持たない
        let f = parse(LOCAL_AGENT).expect("local-agent-mode の形も読めること");
        assert_eq!(f.scheduled_tasks.len(), 3);

        let t = &f.scheduled_tasks[0];
        assert_eq!(t.id, "triage");
        assert_eq!(t.cron_expression.as_deref(), Some("0 * * * *"));
        assert!(!t.enabled);
        assert!(t.cwd.is_none());
        assert!(t.model.is_none());
    }

    #[test]
    fn factory_hint_is_none_when_nothing_is_recorded() {
        // cwd も許可フォルダも無ければ手がかり無し。捏造しない
        let f = parse(LOCAL_AGENT).unwrap();
        assert!(f.scheduled_tasks[0].factory_hint().is_none());
    }

    #[test]
    fn follows_wrapper_to_the_real_spec() {
        // 登録されたSKILL.mdが本体を指すだけのラッパーであることがある。
        // ラッパー本文にはラベルが1つも出てこないため、参照先を解決できないと
        // 推測層は必ず失敗する
        let doc = parse_skill_md(WRAPPER);
        assert!(!doc.body.contains("ai-process:"));

        let referenced = referenced_skill_path(&doc.body).expect("参照先を解決できること");
        assert_eq!(
            referenced,
            "/Users/example/git/beltmap-testfactory/.claude/skills/triage/SKILL.md"
        );
    }

    #[test]
    fn plain_skill_has_no_reference() {
        let doc = parse_skill_md(SKILL);
        assert!(referenced_skill_path(&doc.body).is_none());
    }

    #[test]
    fn joins_both_files() {
        let f = parse(TASKS).unwrap();
        let doc = parse_skill_md(SKILL);
        let m = to_machine(&f.scheduled_tasks[0], Some(&doc));

        assert_eq!(m.runtime, Runtime::DesktopTask);
        assert_eq!(m.summary.as_deref(), Some("ローカル実行検証用"));
        // cwd は工場の所属判定に使うので落とさない
        assert_eq!(m.working_dir.as_deref(), Some("/Users/example/git"));
    }
}
