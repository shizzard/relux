use ratatui::buffer::Buffer;
use ratatui::style::Style;

/// Bounds-checked write of a single character + style to a buffer cell.
pub fn set_cell(x: u16, y: u16, ch: char, style: Style, buf: &mut Buffer) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        let cell = &mut buf[(x, y)];
        cell.set_char(ch);
        cell.set_style(style);
    }
}
