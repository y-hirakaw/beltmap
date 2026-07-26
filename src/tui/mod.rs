//! マップビュー。
//!
//! 骨格(決定論部分)を即時表示する。推測はまだ無いので点線は出ない。
//! **穴があること自体は隠さない**(未解決件数と省略件数を常時表示)。

pub mod model;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::ir::{Ir, LaneRelevance};
use model::{LaneMark, Row};

/// 滞留の警告閾値(日)。configで上書きできるようにするのはPhase 2
const STALE_DAYS: i64 = 3;

pub struct App {
    ir: Ir,
    repo: String,
    rows: Vec<Row>,
    state: ListState,
}

impl App {
    pub fn new(ir: Ir, repo: String) -> Self {
        let rows = model::build_rows(&ir);
        let mut state = ListState::default();
        state.select(rows.iter().position(Row::selectable));
        Self {
            ir,
            repo,
            rows,
            state,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let Some(cur) = self.state.selected() else {
            return;
        };
        let len = self.rows.len() as isize;
        let mut i = cur as isize;
        loop {
            i += delta;
            if i < 0 || i >= len {
                return;
            }
            if self.rows[i as usize].selectable() {
                self.state.select(Some(i as usize));
                return;
            }
        }
    }
}

pub fn run(ir: Ir, repo: String) -> std::io::Result<()> {
    let mut app = App::new(ir, repo);
    let mut terminal = ratatui::init();

    let result = loop {
        if let Err(e) = terminal.draw(|f| draw(f, &mut app)) {
            break Err(e);
        }
        match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                _ => {}
            },
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };

    ratatui::restore();
    result
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        // 枠2行 + 本文2行。3にすると操作ヒントが切れる
        .constraints([Constraint::Min(3), Constraint::Length(4)])
        .split(f.area());

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(outer[0]);

    draw_map(f, app, cols[0]);
    draw_detail(f, app, cols[1]);
    draw_status(f, app, outer[1]);
}

fn draw_map(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app.rows.iter().map(row_to_item).collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" 工場 {} ", app.repo)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut app.state);
}

fn row_to_item(row: &Row) -> ListItem<'static> {
    match row {
        Row::Header(t) => ListItem::new(Line::from(Span::styled(
            format!("{t}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))),

        Row::Lane {
            label,
            count,
            oldest_days,
            mark,
            depth,
        } => {
            let indent = "  ".repeat(*depth);
            let stale = oldest_days.is_some_and(|d| d >= STALE_DAYS);

            let mut spans = vec![
                Span::raw(format!("{indent}")),
                Span::styled(
                    format!("{label:<26}"),
                    match mark {
                        LaneMark::DeadEnd => Style::default().fg(Color::Red),
                        LaneMark::NeedsCheck => Style::default().fg(Color::Yellow),
                        LaneMark::Normal => Style::default(),
                    },
                ),
                Span::styled(
                    format!("{count:>3}件"),
                    if stale {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
            ];

            if let Some(d) = oldest_days {
                spans.push(Span::styled(
                    format!("  最古{d}日"),
                    if stale {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ));
            }

            match mark {
                LaneMark::DeadEnd => spans.push(Span::styled(
                    "  ■行き止まり",
                    Style::default().fg(Color::Red),
                )),
                LaneMark::NeedsCheck => spans.push(Span::styled(
                    "  ?要確認",
                    Style::default().fg(Color::Yellow),
                )),
                LaneMark::Normal => {}
            }

            ListItem::new(Line::from(spans))
        }

        // 観測された流れ。実測なので実線で描く
        Row::Flow { to, note, depth } => {
            let indent = "  ".repeat(depth.saturating_sub(1));
            ListItem::new(Line::from(vec![
                Span::raw(format!("{indent}  └─▶ ")),
                Span::styled(to.clone(), Style::default().fg(Color::Green)),
                Span::styled(
                    format!("   {note}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        }

        Row::Machine {
            name,
            trigger,
            unresolved,
            ..
        } => {
            let mut spans = vec![
                Span::raw("  "),
                Span::styled("🔨 ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{name:<20}"), Style::default()),
                Span::styled(
                    format!("{trigger:<12}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if *unresolved {
                // 推測層が未実行。埋まっていないことを隠さない
                spans.push(Span::styled(
                    "読む/書く ?",
                    Style::default().fg(Color::Yellow),
                ));
            }
            ListItem::new(Line::from(spans))
        }

        // 推測。実測の流れ(└─▶)と見た目を変える
        Row::MachineLabels { reads, writes } => {
            let mut spans = vec![Span::styled(
                "       ┄▶ ",
                Style::default().fg(Color::Magenta),
            )];
            if !reads.is_empty() {
                spans.push(Span::styled(
                    format!("読む {}", reads.join(", ")),
                    Style::default().fg(Color::Magenta),
                ));
            }
            if !writes.is_empty() {
                spans.push(Span::styled(
                    format!("   書く {}", writes.join(", ")),
                    Style::default().fg(Color::Magenta),
                ));
            }
            ListItem::new(Line::from(spans))
        }

        Row::Note(t) => ListItem::new(Line::from(Span::styled(
            format!("  ({t})"),
            Style::default().fg(Color::DarkGray),
        ))),
    }
}

fn draw_detail(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let lines = match app.state.selected().and_then(|i| app.rows.get(i)) {
        Some(Row::Lane { label, .. }) => lane_detail(app, label),
        Some(Row::Machine { id, .. }) => machine_detail(app, id),
        _ => vec![Line::from("—")],
    };

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" 詳細 "))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn lane_detail(app: &App, label: &str) -> Vec<Line<'static>> {
    let Some(lane) = app.ir.lanes.iter().find(|l| l.label == label) else {
        return vec![Line::from("—")];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            lane.label.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("在庫       {}件", lane.count)),
        Line::from(match lane.oldest_days {
            Some(d) => format!("最古滞留   {d}日"),
            None => "最古滞留   —".to_string(),
        }),
        Line::from(""),
        Line::from(Span::styled(
            "レーンだと判断した根拠",
            Style::default().fg(Color::Cyan),
        )),
    ];

    if lane.evidence.is_empty() {
        lines.push(Line::from("  根拠なし"));
    }
    for e in &lane.evidence {
        lines.push(Line::from(format!("  ・{e}")));
    }

    for u in &app.ir.unknowns {
        match u {
            crate::ir::Unknown::OrphanLane { label: l, note } if l == label => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "行き止まり",
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(format!("  {note}")));
            }
            crate::ir::Unknown::UnobservedLane { label: l, note } if l == label => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "要確認",
                    Style::default().fg(Color::Yellow),
                )));
                lines.push(Line::from(format!("  {note}")));
            }
            _ => {}
        }
    }

    lines
}

fn machine_detail(app: &App, id: &str) -> Vec<Line<'static>> {
    let Some(m) = app.ir.machines.iter().find(|m| m.id == id) else {
        return vec![Line::from("—")];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            m.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("{:?}", m.runtime),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
    ];

    if let Some(s) = &m.summary {
        lines.push(Line::from(s.clone()));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(format!(
        "状態       {:?} / {:?}",
        m.status, m.confidence
    )));
    if let Some(w) = &m.working_dir {
        lines.push(Line::from(format!("作業場所   {w}")));
    } else {
        lines.push(Line::from(Span::styled(
            "作業場所   不明",
            Style::default().fg(Color::Yellow),
        )));
    }

    lines.push(Line::from(""));
    if m.reads.is_empty() && m.writes.is_empty() {
        // 推測層が未実行であることを、空欄ではなく理由として書く
        lines.push(Line::from(Span::styled(
            "読む/書くラベルは未解決",
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(Span::styled(
            "  定義本文からの特定は推測層の担当",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(format!("読む   {}", m.reads.join(", "))));
        lines.push(Line::from(format!("書く   {}", m.writes.join(", "))));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "根拠",
        Style::default().fg(Color::Cyan),
    )));
    for p in &m.provenance {
        lines.push(Line::from(format!("  ・{p}")));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// 実スキャン結果があればそれを、無ければ何もしない。
    /// 端末を持たずに描画を検証する。
    fn render(ir: Ir, w: u16, h: u16) -> String {
        let mut app = App::new(ir, "y-hirakaw/beltmap".into());
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();

        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample_ir() -> Ir {
        let raw = std::fs::read(".beltmap/ir.json").expect("先に beltmap scan を実行する");
        serde_json::from_slice(&raw).unwrap()
    }

    #[test]
    #[ignore = "実スキャン結果が要る。cargo test -- --ignored --nocapture で目視する"]
    fn dump_real_map() {
        println!("\n{}\n", render(sample_ir(), 100, 26));
    }

    #[test]
    fn narrow_terminal_does_not_panic() {
        // 工場マシンでは端末幅が読めない。落ちないことだけは担保する
        let ir = Ir {
            version: crate::ir::IR_VERSION,
            scanned_at: chrono::Utc::now(),
            scanned_on: "test".into(),
            machines: Vec::new(),
            lanes: Vec::new(),
            edges: Vec::new(),
            unknowns: Vec::new(),
            answers: Vec::new(),
        };
        let _ = render(ir.clone(), 20, 8);
        let _ = render(ir, 200, 60);
    }
}

fn draw_status(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let hidden = app
        .ir
        .lanes
        .iter()
        .filter(|l| l.relevance != LaneRelevance::Factory)
        .count();
    let unresolved = app
        .ir
        .machines
        .iter()
        .filter(|m| m.reads.is_empty() && m.writes.is_empty())
        .count();

    let mut spans = vec![
        Span::raw(format!(
            "機械 {}  レーン {}  ",
            app.ir.machines.len(),
            app.ir.lanes.len() - hidden
        )),
        Span::styled(
            format!("未解決 {}  ", app.ir.unknowns.len()),
            Style::default().fg(Color::Yellow),
        ),
    ];
    if unresolved > 0 {
        spans.push(Span::styled(
            format!("読む/書く未特定 {unresolved}  "),
            Style::default().fg(Color::Yellow),
        ));
    }
    spans.push(Span::styled(
        format!("スキャン元 {}", app.ir.scanned_on),
        Style::default().fg(Color::DarkGray),
    ));

    let p = Paragraph::new(vec![
        Line::from(spans),
        Line::from(Span::styled(
            "↑↓ 選択   q 終了",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(Block::default().borders(Borders::ALL));

    f.render_widget(p, area);
}
