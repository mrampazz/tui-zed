use std::io::Stdout;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use ratatui::backend::CrosstermBackend;

use crate::event::{self, Action};

pub struct App {
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self { should_quit: false }
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<()> {
        let mut event_stream = EventStream::new();

        loop {
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                Some(Ok(event)) = event_stream.next() => {
                    self.handle_terminal_event(event);
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_terminal_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            if let Some(action) = event::map_key_event(key) {
                match action {
                    Action::Quit => self.should_quit = true,
                }
            }
        }
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let vertical = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
        ]);
        let [main_area, status_area] = vertical.areas(area);

        let welcome = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  tui-zed",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from("  A terminal-based code editor powered by Zed's core."),
            Line::from(""),
            Line::from("  Press 'q' or Ctrl+C to quit."),
        ])
        .block(Block::default().borders(Borders::ALL).title(" tui-zed "));

        frame.render_widget(welcome, main_area);

        let status = Paragraph::new(Line::from(vec![
            Span::styled(" NORMAL ", Style::default().fg(Color::Black).bg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("No file open", Style::default().fg(Color::DarkGray)),
        ]));

        frame.render_widget(status, status_area);
    }
}
