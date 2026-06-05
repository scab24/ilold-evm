
use std::collections::HashSet;
use std::io;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use ilold_core::narrative::trace::{FlowKind, FlowNode, FlowTree};

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

struct ViewerState {
    tree: FlowTree,
    collapsed: HashSet<usize>,
    selected_step_id: usize,
    show_help: bool,
}

impl ViewerState {
    fn new(tree: FlowTree) -> Self {
        let selected_step_id = tree.root.step_id;
        ViewerState {
            tree,
            collapsed: HashSet::new(),
            selected_step_id,
            show_help: false,
        }
    }

    fn flatten(&self) -> Vec<FlatRow<'_>> {
        let mut out = Vec::new();
        flatten_node(&self.tree.root, "", true, true, &self.collapsed, &mut out);
        out
    }

    fn snapshot(&self) -> Vec<RowSnapshot> {
        self.flatten()
            .iter()
            .map(|r| RowSnapshot {
                step_id: r.node.step_id,
                has_children: r.has_children,
                is_collapsed: r.is_collapsed,
            })
            .collect()
    }

    fn cursor_in(&self, flat: &[FlatRow<'_>]) -> usize {
        flat.iter()
            .position(|r| r.node.step_id == self.selected_step_id)
            .unwrap_or(0)
    }

    fn cursor_in_snapshot(&self, snap: &[RowSnapshot]) -> usize {
        snap.iter()
            .position(|r| r.step_id == self.selected_step_id)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy)]
struct RowSnapshot {
    step_id: usize,
    has_children: bool,
    is_collapsed: bool,
}

struct FlatRow<'a> {
    node: &'a FlowNode,
    prefix: String,
    has_children: bool,
    is_collapsed: bool,
}

fn flatten_node<'a>(
    node: &'a FlowNode,
    parent_prefix: &str,
    is_last: bool,
    is_root: bool,
    collapsed: &HashSet<usize>,
    out: &mut Vec<FlatRow<'a>>,
) {
    let connector = if is_root {
        ""
    } else if is_last {
        "└─ "
    } else {
        "├─ "
    };
    let prefix = format!("{}{}", parent_prefix, connector);

    let has_children = !node.children.is_empty();
    let is_collapsed = collapsed.contains(&node.step_id);

    out.push(FlatRow {
        node,
        prefix,
        has_children,
        is_collapsed,
    });

    if has_children && !is_collapsed {
        let extension = if is_root {
            ""
        } else if is_last {
            "   "
        } else {
            "│  "
        };
        let new_prefix = format!("{}{}", parent_prefix, extension);
        let n = node.children.len();
        for (i, child) in node.children.iter().enumerate() {
            flatten_node(child, &new_prefix, i == n - 1, false, collapsed, out);
        }
    }
}

pub fn run_trace_viewer(tree: FlowTree) -> io::Result<()> {
    let _guard = TerminalGuard::new()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut state = ViewerState::new(tree);
    run_loop(&mut terminal, &mut state)?;

    terminal.show_cursor()?;
    Ok(())
}


fn draw_ui(frame: &mut Frame, state: &ViewerState, flat: &[FlatRow<'_>]) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(frame, chunks[0], state);
    draw_list(frame, chunks[1], state, flat);
    draw_footer(frame, chunks[2]);

    if state.show_help {
        draw_help_overlay(frame, area);
    }
}

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let width = area.width.min(64);
    let height = area.height.min(26);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect { x, y, width, height };

    frame.render_widget(Clear, popup);

    let cyan = Style::default().fg(Color::Cyan);
    let gray = Style::default().fg(Color::Gray);
    let yellow = Style::default().fg(Color::Yellow);
    let bold_white = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);

    let lines: Vec<Line> = vec![
        Line::from(Span::styled("Navigation", bold_white)),
        Line::from(vec![
            Span::styled("  ↑/↓  k/j    ", cyan),
            Span::styled("move cursor", gray),
        ]),
        Line::from(vec![
            Span::styled("  g    Home   ", cyan),
            Span::styled("jump to top", gray),
        ]),
        Line::from(vec![
            Span::styled("  G    End    ", cyan),
            Span::styled("jump to bottom", gray),
        ]),
        Line::from(""),
        Line::from(Span::styled("Tree control", bold_white)),
        Line::from(vec![
            Span::styled("  →  Enter  l  ", cyan),
            Span::styled("expand current", gray),
        ]),
        Line::from(vec![
            Span::styled("  ←  h         ", cyan),
            Span::styled("collapse (leaf → parent)", gray),
        ]),
        Line::from(""),
        Line::from(Span::styled("Icons", bold_white)),
        Line::from(vec![
            Span::styled("  ▶ ", yellow),
            Span::styled("collapsed   ", gray),
            Span::styled("▼ ", yellow),
            Span::styled("expanded", gray),
        ]),
        Line::from(vec![
            Span::styled("  ◇ ", gray),
            Span::styled("require     ", gray),
            Span::styled("✏ ", Style::default().fg(Color::Red)),
            Span::styled("write", gray),
        ]),
        Line::from(vec![
            Span::styled("  → ", Style::default().fg(Color::Red)),
            Span::styled("ext call    ", gray),
            Span::styled("○ ", yellow),
            Span::styled("internal call", gray),
        ]),
        Line::from(vec![
            Span::styled("  ◆ ", Style::default().fg(Color::Cyan)),
            Span::styled("emit event  ", gray),
            Span::styled("? ", yellow),
            Span::styled("branch", gray),
        ]),
        Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(Color::Green)),
            Span::styled("return      ", gray),
            Span::styled("✗ ", Style::default().fg(Color::Red)),
            Span::styled("revert", gray),
        ]),
        Line::from(""),
        Line::from(Span::styled("Tags", bold_white)),
        Line::from(vec![
            Span::styled("  [from: name]        ", yellow),
            Span::styled("originated in modifier", gray),
        ]),
        Line::from(vec![
            Span::styled("  [+K ops, depth      ", yellow),
            Span::styled("call body hidden;", gray),
        ]),
        Line::from(vec![
            Span::styled("   limited]           ", yellow),
            Span::styled("use --depth N to see it", gray),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ?  ", cyan),
            Span::styled("toggle this help", gray),
            Span::styled("    q  ", cyan),
            Span::styled("quit", gray),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(cyan)
        .title(Span::styled(" Help ", bold_white));
    let help = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(help, popup);
}

fn draw_header(frame: &mut Frame, area: Rect, state: &ViewerState) {
    let tree = &state.tree;
    let mods = if tree.modifiers.is_empty() {
        "(none)".to_string()
    } else {
        tree.modifiers.join(", ")
    };
    let title = format!(
        " {}::{}  │  modifiers: {}  │  depth: {} ",
        tree.contract, tree.signature, mods, tree.max_depth,
    );
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ilold trace — interactive "),
        );
    frame.render_widget(header, area);
}

fn draw_list(
    frame: &mut Frame,
    area: Rect,
    state: &ViewerState,
    flat: &[FlatRow<'_>],
) {
    let items: Vec<ListItem> = flat.iter().map(format_row).collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(238))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▌ ");

    let mut list_state = ListState::default();
    list_state.select(Some(state.cursor_in(flat)));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_footer(frame: &mut Frame, area: Rect) {
    let hint = Line::from(vec![
        Span::styled(" ↑↓/jk ", Style::default().fg(Color::Cyan)),
        Span::raw("navigate  "),
        Span::styled("→/Enter ", Style::default().fg(Color::Cyan)),
        Span::raw("expand  "),
        Span::styled("← ", Style::default().fg(Color::Cyan)),
        Span::raw("collapse  "),
        Span::styled("g/G ", Style::default().fg(Color::Cyan)),
        Span::raw("top/bot  "),
        Span::styled("? ", Style::default().fg(Color::Cyan)),
        Span::raw("help  "),
        Span::styled("q ", Style::default().fg(Color::Cyan)),
        Span::raw("quit"),
    ]);
    let footer = Paragraph::new(hint).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, area);
}

fn format_row<'a>(row: &FlatRow<'a>) -> ListItem<'a> {
    let collapse_icon = if row.has_children {
        if row.is_collapsed { "▶ " } else { "▼ " }
    } else {
        "  "
    };

    let (kind_icon, kind_text) = crate::fmt::format_flow_label(&row.node.kind);
    let modifier_tag = row.node.from_modifier.as_deref()
        .map(|m| format!("  [from: {}]", m))
        .unwrap_or_default();

    let line = Line::from(vec![
        Span::styled(row.prefix.clone(), Style::default().fg(Color::DarkGray)),
        Span::styled(collapse_icon, Style::default().fg(Color::Yellow)),
        Span::styled(
            kind_icon,
            Style::default().fg(kind_color(&row.node.kind)),
        ),
        Span::raw(" "),
        Span::styled(kind_text, Style::default().fg(kind_text_color(&row.node.kind))),
        Span::styled(modifier_tag, Style::default().fg(Color::Yellow)),
    ]);
    ListItem::new(line)
}

fn kind_color(kind: &FlowKind) -> Color {
    match kind {
        FlowKind::Entry { .. } | FlowKind::EmitEvent { .. } => Color::Cyan,
        FlowKind::Write { .. }
        | FlowKind::StateWrite { .. }
        | FlowKind::ExternalCall { .. }
        | FlowKind::EthTransfer { .. }
        | FlowKind::Revert { .. } => Color::Red,
        FlowKind::InternalCall { .. }
        | FlowKind::BranchTrue { .. }
        | FlowKind::BranchFalse { .. }
        | FlowKind::LoopHeader { .. }
        | FlowKind::AssemblyBlock => Color::Yellow,
        FlowKind::Return => Color::Green,
        _ => Color::Gray,
    }
}

fn kind_text_color(kind: &FlowKind) -> Color {
    match kind {
        FlowKind::Entry { .. } => Color::White,
        FlowKind::Write { .. }
        | FlowKind::StateWrite { .. }
        | FlowKind::ExternalCall { .. }
        | FlowKind::EthTransfer { .. }
        | FlowKind::Revert { .. } => Color::Red,
        FlowKind::InternalCall { .. }
        | FlowKind::BranchTrue { .. }
        | FlowKind::BranchFalse { .. } => Color::Yellow,
        FlowKind::Return => Color::Green,
        _ => Color::Gray,
    }
}


fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ViewerState,
) -> io::Result<()> {
    loop {
        let snap = state.snapshot();
        if !snap.iter().any(|r| r.step_id == state.selected_step_id) {
            if let Some(first) = snap.first() {
                state.selected_step_id = first.step_id;
            }
        }

        {
            let flat = state.flatten();
            terminal.draw(|f| draw_ui(f, state, &flat))?;
        }

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let ev = event::read()?;
        let Event::Key(key) = ev else { continue };
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            continue;
        }

        if !handle_key(state, &snap, key.code) {
            return Ok(());
        }
    }
}

fn handle_key(state: &mut ViewerState, snap: &[RowSnapshot], code: KeyCode) -> bool {
    if state.show_help {
        match code {
            KeyCode::Char('q') => return false,
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::F(1) => {
                state.show_help = false;
            }
            _ => {}
        }
        return true;
    }

    match code {
        KeyCode::Char('q') | KeyCode::Esc => return false,
        KeyCode::Char('?') | KeyCode::F(1) => {
            state.show_help = true;
        }

        KeyCode::Up | KeyCode::Char('k') => {
            let idx = state.cursor_in_snapshot(snap);
            if idx > 0 {
                state.selected_step_id = snap[idx - 1].step_id;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let idx = state.cursor_in_snapshot(snap);
            if idx + 1 < snap.len() {
                state.selected_step_id = snap[idx + 1].step_id;
            }
        }

        KeyCode::Right | KeyCode::Enter | KeyCode::Char('l') => {
            let idx = state.cursor_in_snapshot(snap);
            if let Some(row) = snap.get(idx) {
                if row.has_children && row.is_collapsed {
                    state.collapsed.remove(&row.step_id);
                }
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            let idx = state.cursor_in_snapshot(snap);
            if let Some(row) = snap.get(idx) {
                if row.has_children && !row.is_collapsed {
                    state.collapsed.insert(row.step_id);
                } else if let Some(parent_id) = find_parent_id(&state.tree.root, row.step_id) {
                    state.selected_step_id = parent_id;
                }
            }
        }

        KeyCode::Char('g') | KeyCode::Home => {
            if let Some(first) = snap.first() {
                state.selected_step_id = first.step_id;
            }
        }
        KeyCode::Char('G') | KeyCode::End => {
            if let Some(last) = snap.last() {
                state.selected_step_id = last.step_id;
            }
        }

        _ => {}
    }
    true
}

fn find_parent_id(node: &FlowNode, child_id: usize) -> Option<usize> {
    for child in &node.children {
        if child.step_id == child_id {
            return Some(node.step_id);
        }
        if let Some(found) = find_parent_id(child, child_id) {
            return Some(found);
        }
    }
    None
}
