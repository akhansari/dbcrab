use ansitok::{AnsiColor, ElementKind, Output, VisualAttribute, parse_ansi, parse_ansi_sgr};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

pub(super) fn ansi_to_text(input: &str) -> Text<'static> {
    let mut lines = vec![Line::default()];
    let mut style = Style::default();

    for token in parse_ansi(input) {
        match token.kind() {
            ElementKind::Text => push_styled_text(&mut lines, &input[token.range()], style),
            ElementKind::Sgr => apply_sgr(&mut style, &input[token.range()]),
            _ => {}
        }
    }

    Text::from(lines)
}

fn push_styled_text(lines: &mut Vec<Line<'static>>, text: &str, style: Style) {
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }

        if !line.is_empty()
            && let Some(current_line) = lines.last_mut()
        {
            current_line
                .spans
                .push(Span::styled(line.to_owned(), style));
        }
    }
}

fn apply_sgr(style: &mut Style, sgr: &str) {
    for item in parse_ansi_sgr(sgr) {
        match item {
            Output::Escape(attr) => apply_visual_attribute(style, attr),
            Output::Text(_) => {}
        }
    }
}

fn apply_visual_attribute(style: &mut Style, attr: VisualAttribute) {
    match attr {
        VisualAttribute::Bold => style.add_modifier.insert(Modifier::BOLD),
        VisualAttribute::Faint => style.add_modifier.insert(Modifier::DIM),
        VisualAttribute::Italic => style.add_modifier.insert(Modifier::ITALIC),
        VisualAttribute::Underline | VisualAttribute::DoubleUnderline => {
            style.add_modifier.insert(Modifier::UNDERLINED)
        }
        VisualAttribute::Inverse => style.add_modifier.insert(Modifier::REVERSED),
        VisualAttribute::Hide => style.add_modifier.insert(Modifier::HIDDEN),
        VisualAttribute::Crossedout => style.add_modifier.insert(Modifier::CROSSED_OUT),
        VisualAttribute::FgColor(color) => style.fg = ansi_color(color),
        VisualAttribute::BgColor(color) => style.bg = ansi_color(color),
        VisualAttribute::Reset(code) => apply_sgr_reset(style, code),
        _ => {}
    }
}

fn apply_sgr_reset(style: &mut Style, code: u8) {
    match code {
        0 => *style = Style::default(),
        22 => {
            style.add_modifier.remove(Modifier::BOLD | Modifier::DIM);
            style.sub_modifier.insert(Modifier::BOLD | Modifier::DIM);
        }
        23 => {
            style.add_modifier.remove(Modifier::ITALIC);
            style.sub_modifier.insert(Modifier::ITALIC);
        }
        24 => {
            style.add_modifier.remove(Modifier::UNDERLINED);
            style.sub_modifier.insert(Modifier::UNDERLINED);
        }
        27 => {
            style.add_modifier.remove(Modifier::REVERSED);
            style.sub_modifier.insert(Modifier::REVERSED);
        }
        29 => {
            style.add_modifier.remove(Modifier::CROSSED_OUT);
            style.sub_modifier.insert(Modifier::CROSSED_OUT);
        }
        39 => style.fg = None,
        49 => style.bg = None,
        _ => {}
    }
}

fn ansi_color(color: AnsiColor) -> Option<Color> {
    match color {
        AnsiColor::Bit4(code) => ansi_4bit_color(code),
        AnsiColor::Bit8(code) => Some(Color::Indexed(code)),
        AnsiColor::Bit24 { r, g, b } => Some(Color::Rgb(r, g, b)),
    }
}

fn ansi_4bit_color(code: u8) -> Option<Color> {
    Some(match code {
        30 | 40 => Color::Black,
        31 | 41 => Color::Red,
        32 | 42 => Color::Green,
        33 | 43 => Color::Yellow,
        34 | 44 => Color::Blue,
        35 | 45 => Color::Magenta,
        36 | 46 => Color::Cyan,
        37 | 47 => Color::Gray,
        90 | 100 => Color::DarkGray,
        91 | 101 => Color::LightRed,
        92 | 102 => Color::LightGreen,
        93 | 103 => Color::LightYellow,
        94 | 104 => Color::LightBlue,
        95 | 105 => Color::LightMagenta,
        96 | 106 => Color::LightCyan,
        97 | 107 => Color::White,
        39 | 49 => return None,
        _ => return None,
    })
}
