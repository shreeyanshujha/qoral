//! Drawing: sidebar, agent viewport, status bar, log and help overlays.
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame as RFrame;

use super::{App, Focus, Mode, SIDEBAR_W};
use crate::proto::{attr, Color as PColor, Frame};

pub const SPIN: [&str; 4] = ["◐", "◓", "◑", "◒"];

pub struct Palette {
    pub accent: Color,
    pub idle: Color,
    pub working: Color,
    pub attention: Color,
    pub documenting: Color,
    pub exited: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    pub mail: Color,
    pub bar_bg: Color,
    pub bar_fg: Color,
    pub bar_key: Color,
}

pub fn palette() -> Palette {
    Palette {
        accent: Color::Indexed(214),
        idle: Color::Green,
        working: Color::Yellow,
        attention: Color::Red,
        documenting: Color::Cyan,
        exited: Color::DarkGray,
        text: Color::White,
        dim: Color::DarkGray,
        border: Color::Indexed(238),
        mail: Color::Magenta,
        bar_bg: Color::Indexed(236),
        bar_fg: Color::Indexed(250),
        bar_key: Color::Indexed(245),
    }
}

pub fn harness_color(h: &str, p: &Palette) -> Color {
    match h {
        "claude" => p.accent,
        "codex" => Color::Green,
        "agy" => Color::Magenta,
        "gemini" => Color::Blue,
        "moderator" => Color::Cyan,
        _ => p.text,
    }
}

fn pcolor(c: &PColor) -> Color {
    match c {
        PColor::Default => Color::Reset,
        PColor::Indexed(i) => Color::Indexed(*i),
        PColor::Rgb(r, g, b) => Color::Rgb(*r, *g, *b),
    }
}

pub fn draw(f: &mut RFrame, app: &mut App) {
    let area = f.area();
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
    let cols = Layout::horizontal([Constraint::Length(SIDEBAR_W), Constraint::Length(1), Constraint::Min(1)]).split(rows[0]);
    app.sidebar_area = cols[0];
    app.main_area = cols[2];
    let p = palette();

    match app.mode {
        Mode::Log => draw_log(f, app, cols[0], &p),
        Mode::Help => draw_help(f, app, cols[0], &p),
        _ => draw_sidebar(f, app, cols[0], &p),
    }
    // divider
    let div_style = Style::default().fg(if app.focus == Focus::Main { p.accent } else { p.border });
    for y in cols[1].top()..cols[1].bottom() {
        if let Some(c) = f.buffer_mut().cell_mut(Position::new(cols[1].x, y)) {
            c.set_symbol("│").set_style(div_style);
        }
    }
    draw_main(f, app, cols[2], &p);
    draw_bar(f, app, rows[1], &p);
}

fn truncate(s: &str, w: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if used + cw > w {
            if w > 0 {
                out.pop();
                out.push('…');
            }
            return out;
        }
        out.push(ch);
        used += cw;
    }
    out
}

fn draw_sidebar(f: &mut RFrame, app: &mut App, area: Rect, p: &Palette) {
    let w = area.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let dim = Style::default().fg(p.dim);
    let title_style = Style::default().fg(p.accent).add_modifier(Modifier::BOLD);

    // header
    let mut header = vec![Span::styled(" qoral", title_style)];
    if app.focus == Focus::Sidebar {
        header.push(Span::styled(" ◂", Style::default().fg(p.accent)));
    }
    if app.human_unread > 0 {
        let badge = format!("✉ {}", app.human_unread);
        let pad = w.saturating_sub(7 + badge.len() + 2);
        header.push(Span::raw(" ".repeat(pad)));
        header.push(Span::styled(badge, Style::default().fg(p.mail).add_modifier(Modifier::BOLD)));
    }
    lines.push(Line::from(header));
    lines.push(Line::styled("─".repeat(w), Style::default().fg(p.border)));

    let h = area.height as usize;
    let bus_h = (h * 3 / 10).clamp(4, 10);
    let list_h = h.saturating_sub(2 + 3 + bus_h + 3).max(3);

    if app.agents.is_empty() {
        lines.push(Line::styled("  no agents yet", dim));
        lines.push(Line::from(vec![Span::styled("  press ", dim), Span::styled("n", Style::default().add_modifier(Modifier::BOLD)), Span::styled(" to start one", dim)]));
    }
    let start = app.sel.saturating_sub(list_h.saturating_sub(1)).min(app.agents.len().saturating_sub(list_h));
    for (i, a) in app.agents.iter().enumerate().skip(start).take(list_h) {
        let selected = i == app.sel;
        let shown = app.shown.as_deref() == Some(a.name.as_str());
        let (icon, icon_color) = status_icon(&a.status, app.spin, p);
        let cursor = if selected { "▸" } else { " " };
        let num = if i < 9 { format!("{}", i + 1) } else { " ".into() };
        let name_w = w.saturating_sub(18).max(6);
        let name = format!("{:<name_w$}", truncate(&a.name, name_w));
        let tag = if a.exited_at.is_some() { "exited".to_string() } else if a.harness == "moderator" { "debate".to_string() } else { a.harness.clone() };
        let mail = if a.pending > 0 { format!(" ↓{}", a.pending) } else { String::new() };
        let name_style = if a.exited_at.is_some() {
            Style::default().fg(p.exited)
        } else if shown {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let mut spans = vec![
            Span::styled(cursor, Style::default().fg(p.accent)),
            Span::styled(num, dim),
            Span::raw(" "),
            Span::styled(icon, Style::default().fg(icon_color).add_modifier(if a.status == "attention" { Modifier::BOLD } else { Modifier::empty() })),
            Span::raw(" "),
            Span::styled(name, name_style),
            Span::styled(tag, Style::default().fg(if a.exited_at.is_some() { p.exited } else { harness_color(&a.harness, p) })),
            Span::styled(mail, Style::default().fg(p.mail)),
        ];
        if selected {
            let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
            spans = vec![Span::styled(format!("{:<w$}", truncate(&plain, w)), Style::default().add_modifier(Modifier::REVERSED))];
        }
        lines.push(Line::from(spans));
    }
    while lines.len() < 2 + list_h {
        lines.push(Line::raw(""));
    }

    // details
    lines.push(Line::styled("─".repeat(w), Style::default().fg(p.border)));
    if let Some(a) = app.agents.get(app.sel) {
        lines.push(Line::styled(format!(" {}", truncate(&crate::paths::short_dir(std::path::Path::new(&a.cwd)), w - 1)), dim));
        let task = a.task.as_deref().map(|t| t.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_else(|| "(no task given)".into());
        lines.push(Line::styled(format!(" {}", truncate(&task, w - 1)), dim));
    } else {
        lines.push(Line::raw(""));
        lines.push(Line::raw(""));
    }

    // bus
    lines.push(Line::styled(format!("── bus {}", "─".repeat(w.saturating_sub(7))), Style::default().fg(p.border)));
    let mut bus_lines: Vec<Line> = app
        .bus
        .iter()
        .rev()
        .take(bus_h - 1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| {
            let to_human = m.recipient == "human";
            let col = if to_human { p.mail } else if m.sender == "human" { Color::Cyan } else { p.dim };
            let mark = if to_human && m.read_at.is_none() { Span::styled("•", Style::default().fg(p.mail)) } else { Span::raw(" ") };
            let head = format!("{}→{}", m.sender, m.recipient);
            let body = m.body.split_whitespace().collect::<Vec<_>>().join(" ");
            let rest = w.saturating_sub(1 + head.len() + 1);
            Line::from(vec![mark, Span::styled(head, Style::default().fg(col)), Span::raw(" "), Span::raw(truncate(&body, rest))])
        })
        .collect();
    if bus_lines.is_empty() {
        bus_lines.push(Line::styled(" quiet. agents talk via qoral_send", dim));
    }
    while bus_lines.len() < bus_h - 1 {
        bus_lines.insert(0, Line::raw(""));
    }
    lines.extend(bus_lines);

    // footer
    lines.push(Line::styled("─".repeat(w), Style::default().fg(p.border)));
    let key = |k: &str, l: &str| vec![Span::styled(k.to_string(), Style::default().add_modifier(Modifier::BOLD)), Span::styled(format!(" {l} "), dim)];
    match &app.mode {
        _ if app.flash.is_some() => {
            lines.push(Line::styled(format!(" {}", truncate(&app.flash.as_ref().unwrap().0, w - 1)), Style::default().fg(p.working)));
            lines.push(Line::raw(""));
        }
        Mode::Input { label, value, placeholder, .. } => {
            let label_s = format!(" {label} ");
            let avail = w.saturating_sub(label_s.len() + 1);
            let shown_val: String = if value.is_empty() { placeholder.clone() } else { value.chars().rev().take(avail).collect::<Vec<_>>().into_iter().rev().collect() };
            let mut spans = vec![Span::styled(label_s, Style::default().fg(Color::Cyan))];
            if value.is_empty() {
                spans.push(Span::styled(shown_val, dim));
            } else {
                spans.push(Span::raw(shown_val));
                spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
            }
            lines.push(Line::from(spans));
            lines.push(Line::styled(if value.is_empty() && !placeholder.is_empty() { " Enter confirm · Esc cancel · empty = default" } else { " Enter confirm · Esc cancel" }, dim));
        }
        Mode::Pick { label, options, .. } => {
            lines.push(Line::styled(format!(" {label}"), Style::default().fg(Color::Cyan)));
            let mut spans = vec![Span::raw(" ")];
            for (k, l) in options {
                spans.push(Span::styled(k.to_string(), Style::default().add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(format!(" {l}  "), dim));
            }
            lines.push(Line::from(spans));
        }
        Mode::Confirm { text, .. } => {
            lines.push(Line::styled(format!(" {}", truncate(text, w - 1)), Style::default().fg(p.attention)));
            lines.push(Line::styled(" y confirm · any other key cancels", dim));
        }
        _ => {
            let mut l1 = vec![Span::raw(" ")];
            for (k, l) in [("n", "new"), ("D", "debate"), ("m", "msg"), ("b", "bcast"), ("x", "kill")] {
                l1.extend(key(k, l));
            }
            let mut l2 = vec![Span::raw(" ")];
            for (k, l) in [("l", "log"), ("o", "docs"), ("?", "help"), ("d", "detach"), ("Q", "quit")] {
                l2.extend(key(k, l));
            }
            lines.push(Line::from(l1));
            lines.push(Line::from(l2));
        }
    }
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

pub fn status_icon(status: &str, spin: usize, p: &Palette) -> (String, Color) {
    match status {
        "idle" => ("●".into(), p.idle),
        "working" => (SPIN[spin % SPIN.len()].into(), p.working),
        "attention" => ("!".into(), p.attention),
        "documenting" => ("✎".into(), p.documenting),
        "moderating" => ("⚖".into(), p.documenting),
        "exited" => ("○".into(), p.exited),
        _ => ("◌".into(), p.exited),
    }
}

fn draw_log(f: &mut RFrame, app: &mut App, area: Rect, p: &Palette) {
    let w = area.width as usize;
    let dim = Style::default().fg(p.dim);
    let mut rows: Vec<Line> = Vec::new();
    for m in &app.bus_full {
        let col = if m.recipient == "human" { p.mail } else if m.sender == "human" { Color::Cyan } else { p.working };
        let t = chrono::DateTime::from_timestamp_millis(m.ts).map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
        rows.push(Line::from(vec![
            Span::styled(t, dim),
            Span::raw(" "),
            Span::styled(m.sender.clone(), Style::default().fg(col).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" → {}", m.recipient), Style::default().fg(col)),
        ]));
        for l in wrap_text(&m.body, w.saturating_sub(2)) {
            rows.push(Line::raw(format!("  {l}")));
        }
        rows.push(Line::raw(""));
    }
    if rows.is_empty() {
        rows.push(Line::styled("  no messages yet", dim));
    }
    let body_h = area.height.saturating_sub(3) as usize;
    let max_scroll = rows.len().saturating_sub(body_h);
    app.log_scroll = app.log_scroll.min(max_scroll);
    let from = max_scroll - app.log_scroll;
    let mut lines = vec![
        Line::from(vec![Span::styled(" qoral", Style::default().fg(p.accent).add_modifier(Modifier::BOLD)), Span::styled(" message log", dim)]),
        Line::styled("─".repeat(w), Style::default().fg(p.border)),
    ];
    lines.extend(rows.into_iter().skip(from).take(body_h));
    while lines.len() < area.height as usize - 1 {
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(" j/k ↑/↓ scroll · g/G top/bottom · l/Esc back", dim));
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

fn wrap_text(s: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if cur.is_empty() {
            cur = word.to_string();
        } else if cur.chars().count() + 1 + word.chars().count() <= w {
            cur.push(' ');
            cur.push_str(word);
        } else {
            out.push(cur);
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn draw_help(f: &mut RFrame, _app: &mut App, area: Rect, p: &Palette) {
    let dim = Style::default().fg(p.dim);
    let k = |key: &str, label: &str| Line::from(vec![Span::styled(format!(" {key:<9}"), Style::default().add_modifier(Modifier::BOLD)), Span::styled(label.to_string(), dim)]);
    let sec = |t: &str| Line::styled(format!(" {t}"), Style::default().fg(Color::Cyan));
    let lines = vec![
        Line::from(vec![Span::styled(" qoral", Style::default().fg(p.accent).add_modifier(Modifier::BOLD)), Span::styled(" help", dim)]),
        Line::styled("─".repeat(area.width as usize), Style::default().fg(p.border)),
        sec("sidebar"),
        k("n", "new agent: claude/codex/agy/gemini"),
        k("⏎ / →", "show selected agent on the right"),
        k("j/k ↑/↓", "move selection"),
        k("m", "message selected agent"),
        k("b", "broadcast to all agents"),
        k("x", "kill selected agent"),
        k("l", "message log"),
        k("d", "detach (agents keep running)"),
        k("Q", "quit: kill every agent"),
        Line::raw(""),
        sec("anywhere"),
        k("Alt+←/→", "focus sidebar / agent"),
        k("Alt+h/l", "same"),
        k("Alt+[ ]", "previous / next agent"),
        k("Alt+1..9", "jump to agent N"),
        k("Alt+n", "new agent"),
        k("Alt+m", "message selected agent"),
        k("Alt+d", "detach"),
        Line::raw(""),
        sec("in the agent pane"),
        k("mouse", "wheel scrolls history unless the app wants the mouse"),
        Line::raw(""),
        sec("status"),
        Line::from(vec![Span::styled(" ●", Style::default().fg(p.idle)), Span::styled(" idle at prompt   ", dim), Span::styled("◐", Style::default().fg(p.working)), Span::styled(" working", dim)]),
        Line::from(vec![Span::styled(" !", Style::default().fg(p.attention)), Span::styled(" needs you (permission/login)", dim)]),
        Line::from(vec![Span::styled(" ○", Style::default().fg(p.exited)), Span::styled(" exited (x removes it)", dim)]),
        Line::raw(""),
        Line::styled(" ? or Esc to go back", dim),
    ];
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_main(f: &mut RFrame, app: &mut App, area: Rect, p: &Palette) {
    f.render_widget(Clear, area);
    let Some(frame) = app.frame.as_ref().filter(|fr| Some(&fr.agent) == app.shown.as_ref()) else {
        draw_home(f, area, p);
        return;
    };
    draw_frame(f, frame, area);
    if frame.scroll_offset > 0 {
        let tag = format!(" ↑ {} lines back · scroll down to return ", frame.scroll_offset);
        let x = area.right().saturating_sub(tag.chars().count() as u16 + 1);
        let tag_area = Rect::new(x, area.y, tag.chars().count() as u16, 1);
        f.render_widget(Paragraph::new(tag).style(Style::default().bg(p.accent).fg(Color::Black)), tag_area);
    }
    if app.focus == Focus::Main && frame.cursor_visible {
        if let Some((cx, cy)) = frame.cursor {
            let (x, y) = (area.x + cx, area.y + cy);
            if x < area.right() && y < area.bottom() {
                f.set_cursor_position(Position::new(x, y));
            }
        }
    }
}

pub fn draw_frame(f: &mut RFrame, frame: &Frame, area: Rect) {
    let buf = f.buffer_mut();
    let cols = frame.cols as usize;
    for r in 0..(frame.rows as usize).min(area.height as usize) {
        for c in 0..cols.min(area.width as usize) {
            let cell = &frame.cells[r * cols + c];
            let x = area.x + c as u16;
            let y = area.y + r as u16;
            let Some(b) = buf.cell_mut(Position::new(x, y)) else { continue };
            if cell.attrs & attr::WIDE_SPACER != 0 {
                b.set_symbol(" ");
                continue;
            }
            let mut style = Style::default().fg(pcolor(&cell.fg)).bg(pcolor(&cell.bg));
            let a = cell.attrs;
            if a & attr::BOLD != 0 {
                style = style.add_modifier(Modifier::BOLD);
            }
            if a & attr::DIM != 0 {
                style = style.add_modifier(Modifier::DIM);
            }
            if a & attr::ITALIC != 0 {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if a & attr::UNDERLINE != 0 {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if a & attr::INVERSE != 0 {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if a & attr::STRIKE != 0 {
                style = style.add_modifier(Modifier::CROSSED_OUT);
            }
            if a & attr::HIDDEN != 0 {
                style = style.add_modifier(Modifier::HIDDEN);
            }
            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            b.set_char(ch).set_style(style);
        }
    }
}

fn draw_home(f: &mut RFrame, area: Rect, p: &Palette) {
    let dim = Style::default().fg(p.dim);
    let b = Style::default().add_modifier(Modifier::BOLD);
    let lines = vec![
        Line::raw(""),
        Line::from(vec![Span::styled("  qoral", Style::default().fg(p.accent).add_modifier(Modifier::BOLD)), Span::styled("  many agents, one room", dim)]),
        Line::raw(""),
        Line::raw("  Nothing selected yet. Start an agent from the sidebar on the left:"),
        Line::raw(""),
        Line::from(vec![Span::styled("    n", b), Span::raw("        new agent (pick claude / codex / agy / gemini, a name, a directory, a task)")]),
        Line::from(vec![Span::styled("    Enter", b), Span::raw("    show the selected agent here")]),
        Line::from(vec![Span::styled("    m", b), Span::raw("        message the selected agent      "), Span::styled("b", b), Span::raw("  broadcast to all")]),
        Line::from(vec![Span::styled("    l", b), Span::raw("        open the message log            "), Span::styled("?", b), Span::raw("  full help")]),
        Line::raw(""),
        Line::raw("  From anywhere:"),
        Line::raw(""),
        Line::from(vec![Span::styled("    Alt+←/→", b), Span::raw("  move between sidebar and agent   "), Span::styled("Alt+n", b), Span::raw("  new agent")]),
        Line::from(vec![Span::styled("    Alt+[ ]", b), Span::raw("  previous / next agent            "), Span::styled("Alt+1..9", b), Span::raw("  jump to agent N")]),
        Line::from(vec![Span::styled("    Alt+d", b), Span::raw("    detach (agents keep running; run "), Span::styled("qoral", b), Span::raw(" to come back)")]),
        Line::raw(""),
        Line::raw("  Agents talk to each other through the qoral MCP tools (qoral_send, qoral_wait,"),
        Line::raw("  qoral_spawn, …). Messages to an idle agent are typed straight into its prompt."),
        Line::raw(""),
        Line::styled("  CLI: qoral spawn claude --dir ~/proj \"task\"   qoral send ada \"hi\"   qoral list   qoral log", dim),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn draw_bar(f: &mut RFrame, app: &App, area: Rect, p: &Palette) {
    let key = |k: &str, l: &str| vec![Span::styled(k.to_string(), Style::default().fg(p.bar_key)), Span::styled(format!(" {l}  "), Style::default().fg(p.bar_fg))];
    let mut spans = vec![Span::styled(" qoral", Style::default().fg(p.accent).add_modifier(Modifier::BOLD)), Span::raw("  ")];
    for (k, l) in [("M-n", "new"), ("M-m", "message"), ("M-←/→", "focus sidebar/agent"), ("M-[ M-]", "prev/next agent"), ("M-1..9", "jump"), ("M-d", "detach")] {
        spans.extend(key(k, l));
    }
    if let Some(name) = &app.shown {
        spans.push(Span::styled(format!("  ▸ {name}"), Style::default().fg(p.accent)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(Style::default().bg(p.bar_bg)), area);
}
