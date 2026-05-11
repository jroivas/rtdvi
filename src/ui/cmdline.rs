//! Command-line / message zone at the very bottom of the screen.

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::mode::ModeId;
use crate::Editor;

pub fn render(editor: &Editor, frame: &mut Frame, area: Rect) {
    let text = match editor.mode {
        ModeId::Command => format!(":{}", editor.command_line.input),
        ModeId::Search => {
            // Display either `/pat` or `?pat` (M8 will wire `backward`).
            format!("/{}", editor.search.last_pattern.clone().unwrap_or_default())
        }
        _ => editor.status_message.clone().unwrap_or_default(),
    };
    frame.render_widget(Paragraph::new(Line::from(text)), area);
    if editor.mode == ModeId::Command {
        let x = area.x + 1 + editor.command_line.input[..editor.command_line.cursor].chars().count() as u16;
        let y = area.y;
        frame.set_cursor_position((x, y));
    }
}
