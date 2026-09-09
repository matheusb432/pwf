use std::str::FromStr;

use crate::task::{PriorityTier, order::OrderSpec};

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

const COLOR_CORNFLOWER_BLUE: RgbColor = RgbColor::new(100, 149, 237);
const COLOR_PINK_RED: RgbColor = RgbColor::new(255, 107, 138);
const COLOR_LIME_GREEN: RgbColor = RgbColor::new(163, 230, 53);

const fn resolve_color(value: Option<RgbColor>, default: RgbColor) -> RgbColor {
    match value {
        Some(color) => color,
        None => default,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskStatusColors {
    active: RgbColor,
    done: RgbColor,
    cancelled: RgbColor,
}

impl TaskStatusColors {
    #[must_use]
    pub const fn new(
        active: Option<RgbColor>,
        done: Option<RgbColor>,
        cancelled: Option<RgbColor>,
    ) -> Self {
        Self {
            active: resolve_color(active, COLOR_CORNFLOWER_BLUE),
            done: resolve_color(done, COLOR_LIME_GREEN),
            cancelled: resolve_color(cancelled, COLOR_PINK_RED),
        }
    }

    #[must_use]
    pub const fn active(self) -> RgbColor {
        self.active
    }

    #[must_use]
    pub const fn done(self) -> RgbColor {
        self.done
    }

    #[must_use]
    pub const fn cancelled(self) -> RgbColor {
        self.cancelled
    }
}

impl Default for TaskStatusColors {
    fn default() -> Self {
        Self::new(None, None, None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectStatusColors {
    active: RgbColor,
    paused: RgbColor,
}

impl ProjectStatusColors {
    #[must_use]
    pub const fn new(active: Option<RgbColor>, paused: Option<RgbColor>) -> Self {
        Self {
            active: resolve_color(active, COLOR_CORNFLOWER_BLUE),
            paused: resolve_color(paused, COLOR_PINK_RED),
        }
    }

    #[must_use]
    pub const fn active(self) -> RgbColor {
        self.active
    }

    #[must_use]
    pub const fn paused(self) -> RgbColor {
        self.paused
    }
}

impl Default for ProjectStatusColors {
    fn default() -> Self {
        Self::new(None, None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoteStatusColors {
    active: RgbColor,
    verified: RgbColor,
}

impl NoteStatusColors {
    #[must_use]
    pub const fn new(active: Option<RgbColor>, verified: Option<RgbColor>) -> Self {
        Self {
            active: resolve_color(active, COLOR_CORNFLOWER_BLUE),
            verified: resolve_color(verified, COLOR_LIME_GREEN),
        }
    }

    #[must_use]
    pub const fn active(self) -> RgbColor {
        self.active
    }

    #[must_use]
    pub const fn verified(self) -> RgbColor {
        self.verified
    }
}

impl Default for NoteStatusColors {
    fn default() -> Self {
        Self::new(None, None)
    }
}

/// Immutable, validated user settings used by application operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserSettings {
    task_status_colors: TaskStatusColors,
    project_status_colors: ProjectStatusColors,
    note_status_colors: NoteStatusColors,
    default_priority: PriorityTier,
    default_sort_order: OrderSpec,
}

impl UserSettings {
    #[must_use]
    pub const fn new(
        task_status_colors: TaskStatusColors,
        project_status_colors: ProjectStatusColors,
        note_status_colors: NoteStatusColors,
        default_priority: PriorityTier,
        default_sort_order: OrderSpec,
    ) -> Self {
        Self {
            task_status_colors,
            project_status_colors,
            note_status_colors,
            default_priority,
            default_sort_order,
        }
    }

    #[must_use]
    pub const fn task_status_colors(self) -> TaskStatusColors {
        self.task_status_colors
    }

    #[must_use]
    pub const fn project_status_colors(self) -> ProjectStatusColors {
        self.project_status_colors
    }

    #[must_use]
    pub const fn note_status_colors(self) -> NoteStatusColors {
        self.note_status_colors
    }

    #[must_use]
    pub const fn default_priority(self) -> PriorityTier {
        self.default_priority
    }

    #[must_use]
    pub const fn default_sort_order(self) -> OrderSpec {
        self.default_sort_order
    }
}

impl Default for UserSettings {
    fn default() -> Self {
        Self::new(
            TaskStatusColors::default(),
            ProjectStatusColors::default(),
            NoteStatusColors::default(),
            PriorityTier::Medium,
            OrderSpec::default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::{NoteStatusColors, ProjectStatusColors, RgbColor, TaskStatusColors, UserSettings};
    use crate::task::{PriorityTier, order::OrderSpec};

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
    fn user_settings_resolve_missing_colors_at_construction() {
        let active = RgbColor::from_str("#ff8700").unwrap();
        let colors = TaskStatusColors::new(Some(active), None, None);
        let settings = UserSettings::new(
            colors,
            ProjectStatusColors::default(),
            NoteStatusColors::default(),
            PriorityTier::Medium,
            OrderSpec::default(),
        );

        assert_eq!(settings.task_status_colors().active(), active);
        assert_eq!(
            settings.task_status_colors().done(),
            RgbColor::new(163, 230, 53)
        );
        assert_eq!(
            settings.task_status_colors().cancelled(),
            RgbColor::new(255, 107, 138)
        );

        let defaults = UserSettings::default();
        let blue = RgbColor::new(100, 149, 237);
        let green = RgbColor::new(163, 230, 53);
        let red = RgbColor::new(255, 107, 138);
        assert_eq!(defaults.task_status_colors().active(), blue);
        assert_eq!(defaults.task_status_colors().done(), green);
        assert_eq!(defaults.task_status_colors().cancelled(), red);
        assert_eq!(defaults.project_status_colors().active(), blue);
        assert_eq!(defaults.project_status_colors().paused(), red);
        assert_eq!(defaults.note_status_colors().active(), blue);
        assert_eq!(defaults.note_status_colors().verified(), green);
    }
}
