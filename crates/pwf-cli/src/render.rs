use anstyle::{AnsiColor, Color};
use pwf_models::settings::RgbColor;

pub(crate) fn render_summary(
    identifier: &str,
    title: &str,
    color: impl Into<Color>,
    color_on: bool,
) -> String {
    let identifier = if color_on {
        paint(identifier, color, true)
    } else {
        identifier.to_string()
    };
    format!("{identifier} :: {title}")
}

pub(crate) fn render_confirmation(
    label: &str,
    color: AnsiColor,
    id: &str,
    headline: &str,
    detail_lines: &[String],
    color_on: bool,
) -> String {
    let mut output = format!(
        "{label}: {}\n",
        paint(&format!("{id} {headline}"), color, color_on)
    );
    for line in detail_lines {
        output.push_str(line);
        output.push('\n');
    }
    output
}

pub(crate) const fn rgb_color(color: RgbColor) -> anstyle::RgbColor {
    anstyle::RgbColor(color.red(), color.green(), color.blue())
}

pub(crate) fn paint(text: &str, color: impl Into<Color>, enabled: bool) -> String {
    if !enabled {
        return format!("**{text}**");
    }
    let style = anstyle::Style::new().bold().fg_color(Some(color.into()));
    format!("{}{}{}", style.render(), text, style.render_reset())
}

#[cfg(test)]
mod tests {
    use anstyle::AnsiColor;

    use super::{render_confirmation, render_summary};

    #[test]
    fn plain_summaries_remain_raw_while_mutations_use_emphasis() {
        assert_eq!(
            render_summary("FOO-NOTE-0001", "sample note", AnsiColor::Yellow, false),
            "FOO-NOTE-0001 :: sample note"
        );
        assert_eq!(
            render_confirmation(
                "Added pwf note",
                AnsiColor::Green,
                "FOO-NOTE-0001",
                "foo :: sample note",
                &[],
                false,
            ),
            "Added pwf note: **FOO-NOTE-0001 foo :: sample note**\n"
        );
    }
}
