use ratatui::style::{Color, Modifier, Style};
use text_style::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba};
use tui_syntax_theme::SyntaxTheme;

/// Maps a [`SyntaxTheme`] (which uses [`HighlightStyle`] / [`Hsla`] colors)
/// to ratatui [`Style`] values for terminal rendering.
pub struct TuiTheme {
    styles: Vec<Style>,
}

impl TuiTheme {
    /// Build a [`TuiTheme`] from a [`SyntaxTheme`].
    pub fn from_syntax_theme(theme: &SyntaxTheme) -> Self {
        let styles = theme
            .highlights()
            .iter()
            .map(|hs| highlight_to_ratatui_style(hs))
            .collect();
        Self { styles }
    }

    /// Get the ratatui [`Style`] for a highlight ID (as returned by
    /// `SyntaxTheme::highlight_id`).
    pub fn style_for_id(&self, id: u32) -> Style {
        self.styles
            .get(id as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Get the ratatui [`Style`] for a [`HighlightStyle`] directly.
    pub fn style_for_highlight(highlight: &HighlightStyle) -> Style {
        highlight_to_ratatui_style(highlight)
    }
}

/// Convert an HSLA color to a ratatui truecolor RGB.
fn hsla_to_color(hsla: Hsla) -> Color {
    let rgba = Rgba::from(hsla);
    Color::Rgb(
        (rgba.r * 255.0) as u8,
        (rgba.g * 255.0) as u8,
        (rgba.b * 255.0) as u8,
    )
}

/// Convert a [`HighlightStyle`] to a ratatui [`Style`].
fn highlight_to_ratatui_style(hs: &HighlightStyle) -> Style {
    let mut style = Style::default();

    if let Some(color) = hs.color {
        style = style.fg(hsla_to_color(color));
    }

    if let Some(bg) = hs.background_color {
        style = style.bg(hsla_to_color(bg));
    }

    if let Some(weight) = hs.font_weight {
        if weight >= FontWeight::BOLD {
            style = style.add_modifier(Modifier::BOLD);
        }
    }

    if let Some(font_style) = hs.font_style {
        if font_style == FontStyle::Italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
    }

    if hs.underline.is_some() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }

    if hs.strikethrough.is_some() {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }

    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use text_style::HighlightStyle;
    use tui_syntax_theme::SyntaxTheme;

    #[test]
    fn test_red_keyword_maps_to_rgb() {
        let theme = SyntaxTheme::new_test([("keyword", text_style::red())]);
        let tui_theme = TuiTheme::from_syntax_theme(&theme);

        let style = tui_theme.style_for_id(0);
        // red() is Hsla { h: 0, s: 1, l: 0.5, a: 1 } which is pure red RGB
        assert_eq!(style.fg, Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn test_bold_maps_to_modifier() {
        let hs = HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let style = TuiTheme::style_for_highlight(&hs);
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_italic_maps_to_modifier() {
        let hs = HighlightStyle {
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        };
        let style = TuiTheme::style_for_highlight(&hs);
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn test_unknown_id_returns_default() {
        let theme = SyntaxTheme::new_test([("keyword", text_style::red())]);
        let tui_theme = TuiTheme::from_syntax_theme(&theme);

        let style = tui_theme.style_for_id(999);
        assert_eq!(style, Style::default());
    }
}
