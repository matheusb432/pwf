use anstyle::{Color, RgbColor, Style};

pub const fn color_rgb(red: u8, green: u8, blue: u8) -> Style {
    Style::new()
        .bold()
        .fg_color(Some(Color::Rgb(RgbColor(red, green, blue))))
}

pub fn assert_plain(output: &str) {
    assert!(
        output
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t')),
        "unexpected terminal control characters: {output:?}"
    );
}
