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
