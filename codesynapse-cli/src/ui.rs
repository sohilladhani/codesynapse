use console::{style, Style, Term};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::time::Duration;

pub fn is_tty() -> bool {
    Term::stdout().is_term()
}

fn is_stderr_tty() -> bool {
    Term::stderr().is_term()
}

pub fn ok(msg: &str) {
    println!("  {}  {}", style("✓").green().bold(), msg);
}

pub fn warn(msg: &str) {
    println!("  {}  {}", style("⚠").yellow(), msg);
}

pub fn fail(msg: &str) {
    println!("  {}  {}", style("✗").red(), msg);
}

pub fn info(msg: &str) {
    println!("  {}  {}", style("·").dim(), style(msg).dim());
}

pub fn spinner(label: impl Into<String>) -> ProgressBar {
    let label = label.into();
    if !is_stderr_tty() {
        info(&label);
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan}  {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
    );
    pb.set_message(label);
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

pub fn spin_ok(pb: &ProgressBar, msg: &str) {
    pb.finish_and_clear();
    ok(msg);
}

pub fn spin_warn(pb: &ProgressBar, msg: &str) {
    pb.finish_and_clear();
    warn(msg);
}

pub fn spin_fail(pb: &ProgressBar, msg: &str) {
    pb.finish_and_clear();
    fail(msg);
}

pub fn fmt_duration(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

pub fn fmt_count(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

pub fn print_banner() {
    if !is_tty() {
        return;
    }
    let version = env!("CARGO_PKG_VERSION");
    let b = Style::new().cyan().bold();
    let d = Style::new().dim();
    println!();
    println!(
        "  {}",
        b.apply_to("░█▀▀░░░░█▀█░░░░█▀▄░░░░█▀▀░░░░█▀▀░░░░█░█░░░░█▀█░░░░█▀█░░░░█▀█░░░░█▀▀░░░░█▀▀")
    );
    println!(
        "  {}",
        b.apply_to("░█░░░░░░█░█░░░░█░█░░░░█▀▀░░░░▀▀█░░░░░█░░░░░█░█░░░░█▀█░░░░█▀▀░░░░▀▀█░░░░█▀▀")
    );
    println!(
        "  {}",
        b.apply_to("░▀▀▀░▀░░▀▀▀░▀░░▀▀░░▀░░▀▀▀░▀░░▀▀▀░▀░░░▀░░▀░░▀░▀░▀░░▀░▀░▀░░▀░░░▀░░▀▀▀░▀░░▀▀▀")
    );
    println!();
    println!("  {}  {}", d.apply_to("code intelligence MCP server — gives AI coding assistants architecture-level knowledge of your codebase"), d.apply_to(format!("·  v{}", version)));
    println!();
}

pub fn print_setup_summary(registered: &[String], failed: &[String], hybrid: bool) {
    fn display_name(id: &str) -> &str {
        match id {
            "vscode" => "VS Code",
            "cursor" => "Cursor",
            "windsurf" => "Windsurf",
            "zed" => "Zed",
            "claude" => "Claude",
            other => other,
        }
    }

    let restart_hint = if registered.is_empty() {
        "Re-run with --client to register an editor.".to_string()
    } else {
        let names: Vec<&str> = registered.iter().map(|n| display_name(n)).collect();
        match names.as_slice() {
            [single] => format!("Restart {} to apply changes.", single),
            [a, b] => format!("Restart {} and {} to apply changes.", a, b),
            _ => {
                let (last, rest) = names.split_last().unwrap();
                format!("Restart {} and {} to apply changes.", rest.join(", "), last)
            }
        }
    };

    println!();
    println!(
        "  {}  Ready  ·  {}",
        style("✓").green().bold(),
        style(&restart_hint).dim()
    );
    if !failed.is_empty() {
        let failed_names: Vec<&str> = failed.iter().map(|n| display_name(n)).collect();
        println!(
            "  {}  {} failed — check permissions",
            style("⚠").yellow(),
            failed_names.join(", ")
        );
    }
    if !hybrid {
        println!(
            "  {}  {}",
            style("·").dim(),
            style("Hybrid search disabled — model not found.").dim()
        );
    }
    println!();
}
