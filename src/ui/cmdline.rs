//! Command-line / message zone at the very bottom of the screen.

use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::mode::ModeId;
use crate::Editor;

/// How many rows the cmdline area needs. Grows for multi-line status messages.
pub fn height(editor: &Editor) -> u16 {
    match editor.mode {
        ModeId::Command | ModeId::Search => 1,
        _ => {
            let n = editor
                .status_message
                .as_deref()
                .map(|s| s.lines().count().max(1))
                .unwrap_or(1);
            (n as u16).min(10)
        }
    }
}

pub fn render(editor: &Editor, frame: &mut Frame, area: Rect) {
    let text = match editor.mode {
        ModeId::Command => format!(":{}", editor.command_line.input),
        ModeId::Search => {
            let prefix = if editor.search.direction_forward { '/' } else { '?' };
            format!("{prefix}{}", editor.search.prompt)
        }
        _ => editor.status_message.clone().unwrap_or_default(),
    };
    // `Text::from` splits on '\n', rendering each line in its own row.
    frame.render_widget(Paragraph::new(Text::from(text)), area);
    match editor.mode {
        ModeId::Command => {
            let chars_before =
                editor.command_line.input[..editor.command_line.cursor].chars().count();
            let x = area.x + 1 + chars_before as u16;
            frame.set_cursor_position((x, area.y));
        }
        ModeId::Search => {
            let chars_before =
                editor.search.prompt[..editor.search.prompt_cursor].chars().count();
            let x = area.x + 1 + chars_before as u16;
            frame.set_cursor_position((x, area.y));
        }
        _ => {}
    }
}
