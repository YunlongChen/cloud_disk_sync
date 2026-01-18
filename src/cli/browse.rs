use crate::providers::{FileInfo, StorageProvider};
use chrono::{Local, TimeZone};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{prelude::*, widgets::*};
use std::{io, sync::Arc, time::Duration};

struct AppState {
    current_path: String,
    files: Vec<FileInfo>,
    table_state: TableState,
    loading: bool,
    error: Option<String>,
}

impl AppState {
    fn new(path: String) -> Self {
        Self {
            current_path: path,
            files: vec![],
            table_state: TableState::default(),
            loading: true,
            error: None,
        }
    }

    fn next(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.files.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.files.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn go_up(&mut self) -> Option<String> {
        if self.current_path == "/" {
            return None;
        }

        // Simple string manipulation for paths to avoid OS specific separator issues on WebDAV URLs
        let path = self.current_path.trim_end_matches('/');
        match path.rfind('/') {
            Some(idx) => {
                let parent = if idx == 0 { "/" } else { &path[..idx] };
                Some(parent.to_string())
            }
            None => Some("/".to_string()),
        }
    }
}

pub async fn run_browse_tui(
    provider: Arc<dyn StorageProvider>,
    initial_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Normalize initial path
    let start_path = if initial_path.starts_with('/') {
        initial_path
    } else {
        format!("/{}", initial_path)
    };
    let mut app = AppState::new(start_path);

    // Initial fetch
    fetch_files(&provider, &mut app).await;

    let res = run_app(&mut terminal, &mut app, provider).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn fetch_files(provider: &Arc<dyn StorageProvider>, app: &mut AppState) {
    app.loading = true;
    app.error = None;
    app.files.clear();

    match provider.list(&app.current_path).await {
        Ok(mut files) => {
            // Sort: directories first, then files
            files.sort_by(|a, b| {
                if a.is_dir == b.is_dir {
                    a.path.cmp(&b.path)
                } else {
                    b.is_dir.cmp(&a.is_dir) // true > false (directories first)
                }
            });

            app.files = files;
            if !app.files.is_empty() {
                app.table_state.select(Some(0));
            } else {
                app.table_state.select(None);
            }
        }
        Err(e) => {
            app.error = Some(e.to_string());
        }
    }
    app.loading = false;
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    provider: Arc<dyn StorageProvider>,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == event::KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Down => app.next(),
                KeyCode::Up => app.previous(),
                KeyCode::Enter => {
                    if let Some(selected) = app.table_state.selected()
                        && let Some(file) = app.files.get(selected)
                        && file.is_dir
                    {
                        app.current_path = file.path.clone();
                        fetch_files(&provider, app).await;
                    }
                }
                KeyCode::Backspace | KeyCode::Left => {
                    if let Some(parent) = app.go_up() {
                        app.current_path = parent;
                        fetch_files(&provider, app).await;
                    }
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut AppState) {
    let rects = Layout::default()
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(f.area());

    // Title / Path
    let title = Paragraph::new(format!(" ☁️  Cloud Disk Browser - {}", app.current_path))
        .style(
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(title, rects[0]);

    // Footer
    let footer_text = if app.loading {
        "⏳ 加载中...".to_string()
    } else if let Some(ref err) = app.error {
        format!("❌ 错误: {}", err)
    } else {
        " ↩ 进入文件夹 | ⬆⬇ 浏览 | ⬅ 返回上一级 | q/Esc 退出".to_string()
    };

    let footer_style = if app.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let footer = Paragraph::new(footer_text).style(footer_style);
    f.render_widget(footer, rects[2]);

    // File List
    if app.loading && app.files.is_empty() {
        let loading = Paragraph::new("加载文件中...")
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
            );
        f.render_widget(loading, rects[1]);
    } else {
        // Use a softer highlight style
        let selected_style = Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD);

        let header = ["", "Name", "Size", "Modified"].iter().map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        });

        let selected_index = app.table_state.selected();
        let rows = app.files.iter().enumerate().map(|(i, file)| {
            let is_selected = selected_index == Some(i);

            let (type_icon, type_color) = if file.is_dir {
                ("📁", Color::Blue)
            } else {
                ("📄", Color::White)
            };

            // Format size
            let size_str = if file.is_dir {
                "-".to_string()
            } else {
                format_size(file.size)
            };

            // Format date
            let date_str = if file.modified > 0 {
                match Local.timestamp_opt(file.modified, 0) {
                    chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
                    _ => "-".to_string(),
                }
            } else {
                "-".to_string()
            };

            // Name: Extract filename from path for display, but keep full path for logic
            // Assuming path is like /foo/bar/baz.txt
            let name = if file.path == "/" {
                "/"
            } else {
                file.path
                    .trim_end_matches('/')
                    .split('/')
                    .next_back()
                    .unwrap_or(&file.path)
            };

            // Determine colors based on selection state
            // If selected, use White for better contrast against DarkGray background
            // If not selected, use DarkGray for metadata to keep them subtle
            let metadata_color = if is_selected {
                Color::White
            } else {
                Color::DarkGray
            };
            let name_color = if is_selected {
                Color::White
            } else if file.is_dir {
                Color::Blue
            } else {
                Color::Reset
            };
            let icon_color = if is_selected { type_color } else { type_color }; // Keep icon color or make it white? Let's keep original color for icon

            let cells = vec![
                Cell::from(type_icon).style(Style::default().fg(icon_color)),
                Cell::from(name).style(Style::default().fg(name_color)),
                Cell::from(size_str).style(Style::default().fg(metadata_color)),
                Cell::from(date_str).style(Style::default().fg(metadata_color)),
            ];

            Row::new(cells).height(1).bottom_margin(0)
        });

        let t = Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Percentage(50),
                Constraint::Length(10),
                Constraint::Length(20),
            ],
        )
        .header(Row::new(header).height(1).bottom_margin(1).top_margin(1))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Files "),
        )
        .row_highlight_style(selected_style)
        .highlight_symbol(">> ");

        f.render_stateful_widget(t, rects[1], &mut app.table_state);
    }
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}
