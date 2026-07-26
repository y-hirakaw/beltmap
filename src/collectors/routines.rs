//! クラウドルーチン(5.3)。claude.ai の `GET /v1/code/triggers` を読む。
//!
//! CLIの `/schedule list` は同じエンドポイントを叩いているだけなので、
//! 中間にモデルを挟む必要はない。素のREST GETであり完全な決定論。

use serde::Deserialize;

use crate::ir::{Confidence, Machine, MachineStatus, Runtime, Trigger};

#[derive(Debug, Deserialize)]
pub struct TriggersResponse {
    pub data: Vec<RoutineTrigger>,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
pub struct RoutineTrigger {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cron_expression: Option<String>,
    #[serde(default)]
    pub next_run_at: Option<String>,
    #[serde(default)]
    pub last_fired_at: Option<String>,
    #[serde(default)]
    pub job_config: Option<JobConfig>,
    #[serde(default)]
    pub mcp_connections: Vec<McpConnection>,
}

#[derive(Debug, Deserialize)]
pub struct JobConfig {
    pub ccr: Option<Ccr>,
}

#[derive(Debug, Deserialize)]
pub struct Ccr {
    #[serde(default)]
    pub events: Vec<CcrEvent>,
    #[serde(default)]
    pub session_context: Option<SessionContext>,
}

#[derive(Debug, Deserialize)]
pub struct CcrEvent {
    pub data: Option<CcrEventData>,
}

#[derive(Debug, Deserialize)]
pub struct CcrEventData {
    pub message: Option<CcrMessage>,
}

#[derive(Debug, Deserialize)]
pub struct CcrMessage {
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionContext {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct McpConnection {
    #[serde(default)]
    pub name: String,
}

impl RoutineTrigger {
    /// 機械の仕様書にあたる本文。推測層への入力になる。
    pub fn prompt(&self) -> Option<&str> {
        let ccr = self.job_config.as_ref()?.ccr.as_ref()?;
        let msg = ccr.events.first()?.data.as_ref()?.message.as_ref()?;
        if msg.content.is_empty() {
            None
        } else {
            Some(&msg.content)
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.job_config
            .as_ref()?
            .ccr
            .as_ref()?
            .session_context
            .as_ref()?
            .model
            .as_deref()
    }
}

pub fn parse(raw: &[u8]) -> Result<TriggersResponse, serde_json::Error> {
    serde_json::from_slice(raw)
}

/// ルーチン定義を機械に変換する。
///
/// `reads` / `writes` はここでは埋めない。プロンプト本文からラベルを読み取るのは
/// 決定論では無理であり、推測層(5.2)の担当だからである。空のまま返し、
/// 呼び出し側が enrichment-request に積む。
pub fn to_machine(t: &RoutineTrigger) -> Machine {
    let trigger = match &t.cron_expression {
        Some(cron) => Trigger::Schedule {
            detail: cron.clone(),
        },
        // スケジュールが無いルーチンはAPI/GitHubトリガーで動く
        None => Trigger::Event {
            detail: "api or github".to_string(),
        },
    };

    Machine {
        id: t.id.clone(),
        name: t.name.clone(),
        runtime: Runtime::CloudRoutine,
        trigger,
        reads: Vec::new(),
        writes: Vec::new(),
        // 実際に遷移を起こしたかは transitions と突き合わせて後から判定する。
        // ここでは「定義はある」ところまでしか言えない
        status: if t.enabled {
            MachineStatus::Building
        } else {
            MachineStatus::Planned
        },
        confidence: Confidence::Confirmed,
        provenance: vec![format!("cloud-routine:{}", t.id)],
        summary: None,
        working_dir: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/triggers-list.json");

    #[test]
    fn parses_real_response_shape() {
        let res = parse(FIXTURE).expect("fixture should parse");
        assert_eq!(res.data.len(), 1);

        let t = &res.data[0];
        assert_eq!(t.name, "仕分けループ");
        assert_eq!(t.cron_expression.as_deref(), Some("0 22 * * *"));
        assert!(t.enabled);
        assert_eq!(t.prompt(), Some("ok"));
        assert_eq!(t.model(), Some("claude-sonnet-5"));
        assert_eq!(t.mcp_connections[0].name, "Claude_Code_Remote");
    }

    #[test]
    fn maps_to_machine_without_inventing_labels() {
        let res = parse(FIXTURE).unwrap();
        let m = to_machine(&res.data[0]);

        assert_eq!(m.runtime, Runtime::CloudRoutine);
        assert_eq!(m.confidence, Confidence::Confirmed);
        // 決定論で分からないものを埋めてはならない
        assert!(m.reads.is_empty());
        assert!(m.writes.is_empty());
    }
}
