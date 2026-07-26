//! Desktopローカルタスク(5.3)。
//!
//! 2ファイルの結合で全項目が取れる:
//!   - `claude-code-sessions/*/*/scheduled-tasks.json` … cron / 有効状態 / モデル / cwd
//!   - `filePath` が指す `SKILL.md` … name / description / プロンプト本文
//!
//! パスのUUID2つはハードコードせずグロブで解決する。account単位・
//! インストール単位に分かれるため複数マッチしうる。

use serde::Deserialize;

use crate::ir::{Confidence, Machine, MachineStatus, Runtime, Trigger};

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
    /// この機械が担当している作業フォルダ。どの工場に属するかの手がかりになる
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(rename = "useWorktree", default)]
    pub use_worktree: bool,
}

pub fn parse(raw: &[u8]) -> Result<ScheduledTasksFile, serde_json::Error> {
    serde_json::from_slice(raw)
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
        provenance: vec![task.file_path.clone()],
        summary: doc.and_then(|d| d.description.clone()),
        working_dir: task.cwd.clone(),
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
