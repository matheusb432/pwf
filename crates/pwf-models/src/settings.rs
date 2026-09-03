use std::str::FromStr;

/// An RGB color parsed from the user-settings `#RRGGBB` representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RgbColor {
    red: u8,
    green: u8,
    blue: u8,
}

impl RgbColor {
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    #[must_use]
    pub const fn red(self) -> u8 {
        self.red
    }

    #[must_use]
    pub const fn green(self) -> u8 {
        self.green
    }

    #[must_use]
    pub const fn blue(self) -> u8 {
        self.blue
    }
}

impl FromStr for RgbColor {
    type Err = RgbColorError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' {
            return Err(RgbColorError);
        }

        Ok(Self::new(
            component(bytes[1], bytes[2]).ok_or(RgbColorError)?,
            component(bytes[3], bytes[4]).ok_or(RgbColorError)?,
            component(bytes[5], bytes[6]).ok_or(RgbColorError)?,
        ))
    }
}

fn component(high: u8, low: u8) -> Option<u8> {
    Some(hexadecimal_digit(high)? * 16 + hexadecimal_digit(low)?)
}

fn hexadecimal_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("must use `#RRGGBB` with six hexadecimal digits")]
pub struct RgbColorError;

/// Optional RGB overrides for task lifecycle colors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TaskStatusColors {
    active: Option<RgbColor>,
    done: Option<RgbColor>,
    cancelled: Option<RgbColor>,
}

impl TaskStatusColors {
    #[must_use]
    pub const fn new(
        active: Option<RgbColor>,
        done: Option<RgbColor>,
        cancelled: Option<RgbColor>,
    ) -> Self {
        Self {
            active,
            done,
            cancelled,
        }
    }

    #[must_use]
    pub const fn active(self) -> Option<RgbColor> {
        self.active
    }

    #[must_use]
    pub const fn done(self) -> Option<RgbColor> {
        self.done
    }

    #[must_use]
    pub const fn cancelled(self) -> Option<RgbColor> {
        self.cancelled
    }
}

/// Immutable, validated user settings used by application operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UserSettings {
    task_status_colors: TaskStatusColors,
}

impl UserSettings {
    #[must_use]
    pub const fn new(task_status_colors: TaskStatusColors) -> Self {
        Self { task_status_colors }
    }

    #[must_use]
    pub const fn task_status_colors(self) -> TaskStatusColors {
        self.task_status_colors
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::{RgbColor, TaskStatusColors, UserSettings};

    #[test]
    fn rgb_color_accepts_exact_six_digit_hexadecimal_values() {
        let color = RgbColor::from_str("#1a2B3c").unwrap();

        assert_eq!(color.red(), 0x1a);
        assert_eq!(color.green(), 0x2b);
        assert_eq!(color.blue(), 0x3c);
    }

    #[test]
    fn rgb_color_rejects_values_outside_the_configured_representation() {
        for value in ["000000", "#000", "#00000000", "#gg0000", "#00ff0é"] {
            assert!(RgbColor::from_str(value).is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn user_settings_preserve_optional_task_status_color_overrides() {
        let active = RgbColor::from_str("#ff8700").unwrap();
        let colors = TaskStatusColors::new(Some(active), None, None);
        let settings = UserSettings::new(colors);

        assert_eq!(settings.task_status_colors().active(), Some(active));
        assert_eq!(settings.task_status_colors().done(), None);
        assert_eq!(settings.task_status_colors().cancelled(), None);

        let defaults = UserSettings::default();
        assert_eq!(defaults.task_status_colors(), TaskStatusColors::default());
    }
}
