//! UI zoom: the theme's font sizes, stepped and clamped.

/// gpui-component's default UI and monospace font sizes.
pub const DEFAULT: f32 = 16.0;
const DEFAULT_MONO: f32 = 13.0;
const STEP: f32 = 1.5;
const MIN: f32 = 8.0;
const MAX: f32 = 36.0;

/// The UI font size after zooming `steps` (negative zooms out).
pub fn step(size: f32, steps: i32) -> f32 {
    (size + STEP * steps as f32).clamp(MIN, MAX)
}

/// The monospace size that keeps the default ratio to the UI size.
pub fn mono(size: f32) -> f32 {
    size * DEFAULT_MONO / DEFAULT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_within_bounds() {
        assert_eq!(step(DEFAULT, 1), 17.5);
        assert_eq!(step(DEFAULT, -2), 13.0);
        assert_eq!(step(35.0, 1), MAX);
        assert_eq!(step(9.0, -1), MIN);
    }

    #[test]
    fn mono_keeps_default_ratio() {
        assert_eq!(mono(DEFAULT), DEFAULT_MONO);
        assert_eq!(mono(32.0), 26.0);
    }
}
