use std::io;
use std::os::unix::io::AsRawFd;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::TableState;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Terminal;

use crate::models::*;
use crate::tui::data::*;
use crate::tui::input::InputField;
use crate::tui::report_tab::render_report_tab;
use crate::tui::stat_tab::render_stat_tab;
use crate::tui::usage_tab::render_usage_tab;
use crate::tui::widgets::render_help_popup;
use crate::tui::TuiConfig;

#[derive(PartialEq, Clone, Copy)]
pub enum ActiveTab {
    Report,
    Usage,
    Stat,
}

const STATE_FILTERS: &[&str] = &[
    "all",
    "RUNNING",
    "PENDING",
    "COMPLETED",
    "FAILED",
    "CANCELLED",
];

pub struct App {
    pub active_tab: ActiveTab,
    pub search: InputField,
    pub show_help: bool,
    pub paused: bool,
    pub refresh_interval: u64,
    pub seconds_until_refresh: u64,
    pub last_tick: Instant,

    // Data
    pub report_metrics: Vec<GPUMetrics>,
    pub report_loading: bool,
    pub report_error: Option<String>,

    pub gpu_summaries: Vec<GPUTypeSummary>,
    pub cpu_summary: CPUTypeSummary,
    pub queue_summary: QueueSummary,
    pub usage_loading: bool,
    pub usage_error: Option<String>,

    pub stat_metrics: Vec<GPUMetrics>,
    pub stat_loading: bool,
    pub stat_error: Option<String>,

    // Table state
    pub report_table_state: TableState,
    pub stat_table_state: TableState,

    // Per-tab state filters
    pub report_state_filter: String,
    pub stat_state_filter: String,

    // Config
    pub config: TuiConfig,

    // Channel
    pub tx: mpsc::Sender<DataMessage>,
    pub rx: mpsc::Receiver<DataMessage>,
}

impl App {
    pub fn new(config: TuiConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let refresh_interval = config.refresh_interval;
        Self {
            active_tab: ActiveTab::Report,
            search: InputField::new(),
            show_help: false,
            paused: false,
            refresh_interval,
            seconds_until_refresh: refresh_interval,
            last_tick: Instant::now(),

            report_metrics: Vec::new(),
            report_loading: true,
            report_error: None,

            gpu_summaries: Vec::new(),
            cpu_summary: CPUTypeSummary {
                total_cpus: 0,
                used_cpus: 0,
                available_cpus: 0,
                avg_cpu_load: 0.0,
                nodes_with_cpus: Vec::new(),
            },
            queue_summary: QueueSummary {
                total_jobs: 0,
                total_cpus: 0,
                total_gpus: 0,
                jobs_by_user: std::collections::HashMap::new(),
                jobs_by_partition: std::collections::HashMap::new(),
                jobs_by_reason: std::collections::HashMap::new(),
            },
            usage_loading: true,
            usage_error: None,

            stat_metrics: Vec::new(),
            stat_loading: true,
            stat_error: None,

            report_table_state: TableState::default(),
            stat_table_state: TableState::default(),

            report_state_filter: "all".to_string(),
            stat_state_filter: "RUNNING".to_string(),

            config,
            tx,
            rx,
        }
    }

    pub fn cycle_state_filter(&mut self) {
        let filter = match self.active_tab {
            ActiveTab::Report => &mut self.report_state_filter,
            ActiveTab::Stat => &mut self.stat_state_filter,
            ActiveTab::Usage => return,
        };
        let current_idx = STATE_FILTERS
            .iter()
            .position(|&s| s.eq_ignore_ascii_case(filter))
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % STATE_FILTERS.len();
        *filter = STATE_FILTERS[next_idx].to_string();
    }

    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            ActiveTab::Report => ActiveTab::Usage,
            ActiveTab::Usage => ActiveTab::Stat,
            ActiveTab::Stat => ActiveTab::Report,
        };
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            ActiveTab::Report => ActiveTab::Stat,
            ActiveTab::Usage => ActiveTab::Report,
            ActiveTab::Stat => ActiveTab::Usage,
        };
    }

    pub fn trigger_refresh(&mut self) {
        self.seconds_until_refresh = self.refresh_interval;
        self.report_loading = true;
        self.usage_loading = true;
        self.stat_loading = true;
        self.report_error = None;
        self.usage_error = None;
        self.stat_error = None;

        fetch_report_async(
            self.tx.clone(),
            self.config.partition.clone(),
            self.config.user.clone(),
            self.config.starttime.clone(),
        );
        fetch_usage_async(self.tx.clone(), self.config.partitions.clone());

        let stat_user = self.config.effective_user.clone();
        fetch_stat_async(self.tx.clone(), stat_user, self.config.partition.clone());
    }

    pub fn handle_data_message(&mut self, msg: DataMessage) {
        match msg {
            DataMessage::ReportReady(data) => {
                self.report_metrics = data.metrics;
                self.report_error = data.error;
                self.report_loading = false;
            }
            DataMessage::UsageReady(data) => {
                self.gpu_summaries = data.gpu_summaries;
                self.cpu_summary = data.cpu_summary;
                self.queue_summary = data.queue_summary;
                self.usage_error = data.error;
                self.usage_loading = false;
            }
            DataMessage::StatReady(data) => {
                self.stat_metrics = data.metrics;
                self.stat_error = data.error;
                self.stat_loading = false;
            }
        }
    }

    fn filtered_report_count(&self) -> usize {
        let s = self.search.value.to_lowercase();
        let state_filter = &self.report_state_filter;
        self.report_metrics
            .iter()
            .filter(|m| {
                if state_filter != "all" && !m.state.matches_filter(state_filter) {
                    return false;
                }
                if !s.is_empty() {
                    return m.user.to_lowercase().contains(&s)
                        || m.job_id.to_string().contains(&s)
                        || m.state.contains_search(&s)
                        || m.partition.to_lowercase().contains(&s);
                }
                true
            })
            .count()
    }

    fn filtered_stat_count(&self) -> usize {
        let s = self.search.value.to_lowercase();
        let state_filter = &self.stat_state_filter;
        self.stat_metrics
            .iter()
            .filter(|m| {
                if state_filter != "all" && !m.state.matches_filter(state_filter) {
                    return false;
                }
                if !s.is_empty() {
                    return m.user.to_lowercase().contains(&s)
                        || m.job_id.to_string().contains(&s)
                        || m.partition.to_lowercase().contains(&s);
                }
                true
            })
            .count()
    }

    pub fn navigate_up(&mut self) {
        let count = match self.active_tab {
            ActiveTab::Report => self.filtered_report_count(),
            ActiveTab::Stat => self.filtered_stat_count(),
            ActiveTab::Usage => return,
        };
        if count == 0 {
            return;
        }
        let state = match self.active_tab {
            ActiveTab::Report => &mut self.report_table_state,
            ActiveTab::Stat => &mut self.stat_table_state,
            ActiveTab::Usage => return,
        };
        let i = match state.selected() {
            Some(i) => {
                if i == 0 {
                    count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        state.select(Some(i));
    }

    pub fn navigate_down(&mut self) {
        let count = match self.active_tab {
            ActiveTab::Report => self.filtered_report_count(),
            ActiveTab::Stat => self.filtered_stat_count(),
            ActiveTab::Usage => return,
        };
        if count == 0 {
            return;
        }
        let state = match self.active_tab {
            ActiveTab::Report => &mut self.report_table_state,
            ActiveTab::Stat => &mut self.stat_table_state,
            ActiveTab::Usage => return,
        };
        let i = match state.selected() {
            Some(i) => {
                if i >= count - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        state.select(Some(i));
    }

    pub fn navigate_home(&mut self) {
        let state = match self.active_tab {
            ActiveTab::Report => &mut self.report_table_state,
            ActiveTab::Stat => &mut self.stat_table_state,
            ActiveTab::Usage => return,
        };
        state.select(Some(0));
    }

    pub fn navigate_end(&mut self) {
        let count = match self.active_tab {
            ActiveTab::Report => self.filtered_report_count(),
            ActiveTab::Stat => self.filtered_stat_count(),
            ActiveTab::Usage => return,
        };
        if count == 0 {
            return;
        }
        let state = match self.active_tab {
            ActiveTab::Report => &mut self.report_table_state,
            ActiveTab::Stat => &mut self.stat_table_state,
            ActiveTab::Usage => return,
        };
        state.select(Some(count - 1));
    }
}

// Redirect stderr to /dev/null so background thread eprintln! calls
// don't corrupt the alternate screen buffer.
fn suppress_stderr() -> Option<i32> {
    extern "C" {
        fn dup(fd: i32) -> i32;
        fn dup2(oldfd: i32, newfd: i32) -> i32;
    }
    let devnull = std::fs::File::open("/dev/null").ok()?;
    let stderr_fd = io::stderr().as_raw_fd();
    let saved = unsafe { dup(stderr_fd) };
    if saved < 0 {
        return None;
    }
    unsafe { dup2(devnull.as_raw_fd(), stderr_fd) };
    Some(saved)
}

fn restore_stderr(saved_fd: i32) {
    extern "C" {
        fn dup2(oldfd: i32, newfd: i32) -> i32;
        fn close(fd: i32) -> i32;
    }
    let stderr_fd = io::stderr().as_raw_fd();
    unsafe {
        dup2(saved_fd, stderr_fd);
        close(saved_fd);
    }
}

pub fn run_app(config: TuiConfig) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Suppress stderr so background thread progress output
    // doesn't corrupt the TUI display
    let saved_stderr = suppress_stderr();

    let mut app = App::new(config);
    app.trigger_refresh();

    let tick_rate = Duration::from_millis(100);

    loop {
        terminal.draw(|f| draw_ui(f, &mut app))?;

        // Check for data messages
        while let Ok(msg) = app.rx.try_recv() {
            app.handle_data_message(msg);
        }

        // Handle tick timer
        if app.last_tick.elapsed() >= Duration::from_secs(1) {
            app.last_tick = Instant::now();
            if !app.paused {
                if app.seconds_until_refresh > 0 {
                    app.seconds_until_refresh -= 1;
                }
                if app.seconds_until_refresh == 0 {
                    app.trigger_refresh();
                }
            }
        }

        // Poll for events
        if event::poll(tick_rate)? {
            let ev = event::read()?;

            // Terminal resize: just loop back so draw_ui() picks up the new size
            if matches!(ev, Event::Resize(_, _)) {
                continue;
            }

            if let Event::Key(key) = ev {
                // Help popup intercepts all keys
                if app.show_help {
                    app.show_help = false;
                    continue;
                }

                // Search input mode
                if app.search.focused {
                    match key.code {
                        KeyCode::Esc => {
                            app.search.clear();
                            app.search.focused = false;
                        }
                        KeyCode::Enter => {
                            app.search.focused = false;
                        }
                        KeyCode::Backspace => app.search.handle_backspace(),
                        KeyCode::Delete => app.search.handle_delete(),
                        KeyCode::Left => app.search.move_left(),
                        KeyCode::Right => app.search.move_right(),
                        KeyCode::Home => app.search.home(),
                        KeyCode::End => app.search.end(),
                        KeyCode::Char(c) => app.search.handle_char(c),
                        _ => {}
                    }
                    continue;
                }

                // Normal mode key handling
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('1') => app.active_tab = ActiveTab::Report,
                    KeyCode::Char('2') => app.active_tab = ActiveTab::Usage,
                    KeyCode::Char('3') => app.active_tab = ActiveTab::Stat,
                    KeyCode::Tab => app.next_tab(),
                    KeyCode::BackTab => app.prev_tab(),
                    KeyCode::Char('r') => {
                        app.trigger_refresh();
                    }
                    KeyCode::Char('p') => {
                        app.paused = !app.paused;
                    }
                    KeyCode::Char('s') => {
                        app.cycle_state_filter();
                    }
                    KeyCode::Char('+') | KeyCode::Char('=') => {
                        app.refresh_interval = (app.refresh_interval + 5).min(300);
                        if app.seconds_until_refresh > app.refresh_interval {
                            app.seconds_until_refresh = app.refresh_interval;
                        }
                    }
                    KeyCode::Char('-') => {
                        app.refresh_interval = app.refresh_interval.saturating_sub(5).max(5);
                        if app.seconds_until_refresh > app.refresh_interval {
                            app.seconds_until_refresh = app.refresh_interval;
                        }
                    }
                    KeyCode::Char('/') => {
                        app.search.focused = true;
                    }
                    KeyCode::Char('?') => {
                        app.show_help = true;
                    }
                    KeyCode::Esc => {
                        if !app.search.is_empty() {
                            app.search.clear();
                        }
                    }
                    KeyCode::Up => app.navigate_up(),
                    KeyCode::Down => app.navigate_down(),
                    KeyCode::Home => app.navigate_home(),
                    KeyCode::End => app.navigate_end(),
                    _ => {}
                }
            }
        }
    }

    // Restore stderr before leaving TUI
    if let Some(fd) = saved_stderr {
        restore_stderr(fd);
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw_ui(f: &mut ratatui::Frame, app: &mut App) {
    let show_search = app.search.focused || !app.search.is_empty();
    let search_height = if show_search { 1 } else { 0 };

    let chunks = Layout::vertical([
        Constraint::Length(3),             // Tab bar
        Constraint::Length(1),             // Status line
        Constraint::Length(search_height), // Search bar (only when active)
        Constraint::Min(5),                // Content
        Constraint::Length(1),             // Footer
    ])
    .split(f.area());

    draw_tab_bar(f, chunks[0], app);
    draw_status_line(f, chunks[1], app);
    if show_search {
        draw_search_bar(f, chunks[2], app);
    }
    draw_content(f, chunks[3], app);
    draw_footer(f, chunks[4], app);

    if app.show_help {
        render_help_popup(f);
    }
}

fn draw_tab_bar(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let titles = vec!["[1] Report", "[2] Usage", "[3] My Jobs"];
    let selected = match app.active_tab {
        ActiveTab::Report => 0,
        ActiveTab::Usage => 1,
        ActiveTab::Stat => 2,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .title(" Slurm GPU Monitor ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .select(selected)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}

fn draw_status_line(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let refresh_text = if app.paused {
        Span::styled(
            "PAUSED",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(
                "Refresh in {}s ({}s interval)",
                app.seconds_until_refresh, app.refresh_interval
            ),
            Style::default().fg(Color::DarkGray),
        )
    };

    let state_filter = match app.active_tab {
        ActiveTab::Report => &app.report_state_filter,
        ActiveTab::Stat => &app.stat_state_filter,
        ActiveTab::Usage => &"all".to_string(),
    };
    let filter_text = if state_filter != "all" {
        Span::styled(
            format!("  Filter: {}", state_filter),
            Style::default().fg(Color::Yellow),
        )
    } else {
        Span::styled("  Filter: all", Style::default().fg(Color::DarkGray))
    };

    let loading_indicator = if app.report_loading || app.usage_loading || app.stat_loading {
        Span::styled(" [loading...]", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };

    let line = Line::from(vec![
        Span::raw(" "),
        refresh_text,
        filter_text,
        Span::styled(
            "  (s filter, +/- interval, ? help)",
            Style::default().fg(Color::DarkGray),
        ),
        loading_indicator,
    ]);

    f.render_widget(Paragraph::new(line), area);
}

fn draw_search_bar(f: &mut ratatui::Frame, area: Rect, app: &App) {
    if app.search.focused || !app.search.is_empty() {
        let style = if app.search.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let cursor_indicator = if app.search.focused { "_" } else { "" };
        let line = Line::from(vec![
            Span::styled(" Search: ", style.add_modifier(Modifier::BOLD)),
            Span::styled(app.search.value.clone(), style),
            Span::styled(cursor_indicator, Style::default().fg(Color::White)),
            Span::styled("  (Esc to clear)", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(line), area);
    }
}

fn draw_content(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    match app.active_tab {
        ActiveTab::Report => {
            render_report_tab(
                f,
                area,
                &app.report_metrics,
                &app.search.value,
                &app.report_state_filter,
                &mut app.report_table_state,
                app.report_loading,
                app.report_error.as_deref(),
            );
        }
        ActiveTab::Usage => {
            render_usage_tab(
                f,
                area,
                &app.gpu_summaries,
                &app.cpu_summary,
                &app.queue_summary,
                app.usage_loading,
                app.usage_error.as_deref(),
            );
        }
        ActiveTab::Stat => {
            render_stat_tab(
                f,
                area,
                &app.stat_metrics,
                &app.search.value,
                &app.stat_state_filter,
                &mut app.stat_table_state,
                app.stat_loading,
                app.stat_error.as_deref(),
            );
        }
    }
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let partition_info = app.config.partition.as_deref().unwrap_or("all");
    let user_info = app.config.user.as_deref().unwrap_or("all");

    let job_count = match app.active_tab {
        ActiveTab::Report => app.report_metrics.len(),
        ActiveTab::Usage => app.gpu_summaries.len(),
        ActiveTab::Stat => app.stat_metrics.len(),
    };

    let line = Line::from(vec![Span::styled(
        format!(
            " Items: {} | Partition: {} | User: {} ",
            job_count, partition_info, user_info
        ),
        Style::default().fg(Color::DarkGray),
    )]);

    f.render_widget(Paragraph::new(line), area);
}
