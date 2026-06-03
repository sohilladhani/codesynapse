use codesynapse_tui::{Tab, TuiApp};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph, Row, Table, Tabs,
    },
    Frame,
};

pub fn render(f: &mut Frame, app: &TuiApp) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(f, app, chunks[0]);

    match app.selected_tab {
        Tab::Overview => render_overview(f, app, chunks[1]),
        Tab::Nodes => render_nodes(f, app, chunks[1]),
        Tab::Edges => render_edges(f, app, chunks[1]),
    }

    render_help(f, app, chunks[2]);
}

fn render_tabs(f: &mut Frame, app: &TuiApp, area: Rect) {
    let titles: Vec<Line> = Tab::all().iter().map(|t| Line::from(t.title())).collect();
    let selected = match app.selected_tab {
        Tab::Overview => 0,
        Tab::Nodes => 1,
        Tab::Edges => 2,
    };
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" codesynapse "),
        )
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}

fn render_overview(f: &mut Frame, app: &TuiApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_stats_panel(f, app, chunks[0]);
    render_language_chart(f, app, chunks[1]);
}

fn render_stats_panel(f: &mut Frame, app: &TuiApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    let summary = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Nodes : "),
            Span::styled(
                app.stats.total_nodes.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Edges : "),
            Span::styled(
                app.stats.total_edges.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Langs : "),
            Span::styled(
                app.stats.language_counts.len().to_string(),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Stats "));
    f.render_widget(summary, chunks[0]);

    let rows: Vec<Row> = app
        .stats
        .top_nodes_by_degree
        .iter()
        .map(|(label, deg)| Row::new(vec![label.as_str().to_string(), deg.to_string()]))
        .collect();
    let table = Table::new(rows, [Constraint::Min(20), Constraint::Length(8)])
        .header(
            Row::new(vec!["Node", "Degree"]).style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Yellow),
            ),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Top Nodes by Degree "),
        );
    f.render_widget(table, chunks[1]);
}

fn render_language_chart(f: &mut Frame, app: &TuiApp, area: Rect) {
    let mut pairs: Vec<(&str, u64)> = app
        .stats
        .language_counts
        .iter()
        .map(|(k, &v)| (k.as_str(), v as u64))
        .collect();
    pairs.sort_by_key(|k| std::cmp::Reverse(k.1));
    pairs.truncate(12);

    let bars: Vec<Bar> = pairs
        .iter()
        .map(|(label, value)| {
            Bar::default()
                .value(*value)
                .label(Line::from(*label))
                .style(Style::default().fg(Color::Cyan))
        })
        .collect();

    let group = BarGroup::default().bars(&bars);
    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Language Breakdown "),
        )
        .data(group)
        .bar_width(3)
        .bar_gap(1)
        .value_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .label_style(Style::default().fg(Color::Gray));
    f.render_widget(chart, area);
}

fn render_nodes(f: &mut Frame, app: &TuiApp, area: Rect) {
    let filtered = app.filtered_nodes();
    let items: Vec<ListItem> = filtered
        .iter()
        .skip(app.scroll_offset)
        .map(|n| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<30}", truncate(&n.label, 29)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(" {:<10}", truncate(&n.file_type, 9)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!(" deg:{:<4}", n.degree()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!(" {}", truncate(&n.source_file, 40)),
                    Style::default().fg(Color::Gray),
                ),
            ]))
        })
        .collect();

    let filter_display = if app.filter.is_empty() {
        format!(" Nodes ({}) ", filtered.len())
    } else {
        format!(" Nodes ({}) [filter: {}] ", filtered.len(), app.filter)
    };

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(filter_display));
    f.render_widget(list, area);
}

fn render_edges(f: &mut Frame, app: &TuiApp, area: Rect) {
    let items: Vec<ListItem> = app
        .edge_list
        .iter()
        .skip(app.scroll_offset)
        .map(|e| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<30}", truncate(&e.source, 29)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!(" ──{:<14}──▶ ", truncate(&e.relation, 12)),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    truncate(&e.target, 30).to_string(),
                    Style::default().fg(Color::Green),
                ),
            ]))
        })
        .collect();

    let title = format!(" Edges ({}) ", app.edge_list.len());
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(list, area);
}

fn render_help(f: &mut Frame, app: &TuiApp, area: Rect) {
    let help = match app.selected_tab {
        Tab::Nodes => {
            " [q] quit  [Tab/Shift+Tab] switch tab  [↑↓] scroll  [/] filter  [Esc] clear filter "
        }
        _ => " [q] quit  [Tab/Shift+Tab] switch tab  [↑↓] scroll ",
    };
    let para = Paragraph::new(help)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(para, area);
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
