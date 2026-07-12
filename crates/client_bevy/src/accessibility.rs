//! First-playable accessibility effect controls (`GB-M01-10A`).
//!
//! These settings are presentation-only. They never enter the fixed simulation or its hash.

use std::env;

use bevy::prelude::*;

use crate::{FrameSet, combat::ProjectilePresentation, enemies::HostileProjectilePresentation};

const OVERLAY_TOGGLE: KeyCode = KeyCode::F6;
const ACCESSIBILITY_EVIDENCE_ENV: &str = "GRAVEBOUND_ACCESSIBILITY_EVIDENCE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostileTheme {
    ShapeAndColor,
    ColorblindSafe,
    ThickOutline,
}

impl HostileTheme {
    const fn next(self) -> Self {
        match self {
            Self::ShapeAndColor => Self::ColorblindSafe,
            Self::ColorblindSafe => Self::ThickOutline,
            Self::ThickOutline => Self::ShapeAndColor,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ShapeAndColor => "SHAPE + COLOR",
            Self::ColorblindSafe => "COLORBLIND SAFE",
            Self::ThickOutline => "THICK OUTLINE",
        }
    }
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AccessibilitySettings {
    pub(crate) screen_shake_percent: u8,
    pub(crate) flash_intensity_percent: u8,
    pub(crate) friendly_opacity_percent: u8,
    pub(crate) reduced_motion: bool,
    pub(crate) high_contrast_telegraphs: bool,
    pub(crate) hostile_theme: HostileTheme,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            screen_shake_percent: 50,
            flash_intensity_percent: 50,
            friendly_opacity_percent: 35,
            reduced_motion: false,
            high_contrast_telegraphs: false,
            hostile_theme: HostileTheme::ShapeAndColor,
        }
    }
}

impl AccessibilitySettings {
    pub(crate) fn set_screen_shake(&mut self, percent: u8) -> Result<(), &'static str> {
        if percent > 100 {
            return Err("screen shake must be within 0..=100");
        }
        self.screen_shake_percent = percent;
        Ok(())
    }

    pub(crate) fn set_flash_intensity(&mut self, percent: u8) -> Result<(), &'static str> {
        if percent > 100 {
            return Err("flash intensity must be within 0..=100");
        }
        self.flash_intensity_percent = percent;
        Ok(())
    }

    pub(crate) fn set_friendly_opacity(&mut self, percent: u8) -> Result<(), &'static str> {
        if !(10..=60).contains(&percent) {
            return Err("friendly opacity must be within 10..=60");
        }
        self.friendly_opacity_percent = percent;
        Ok(())
    }

    /// Hostile mechanics are never eligible for accessibility culling.
    pub(crate) const fn hostile_mechanics_visible() -> bool {
        true
    }
}

#[derive(Component)]
struct AccessibilityOverlay;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct HostileOutlineBaseSize(pub(crate) Vec2);

#[derive(Resource)]
struct AccessibilityEvidence(bool);

pub(crate) fn configure(app: &mut App) {
    let evidence_preset = env::var(ACCESSIBILITY_EVIDENCE_ENV).ok();
    let evidence = evidence_preset.is_some();
    let settings = if evidence {
        AccessibilitySettings {
            screen_shake_percent: 0,
            flash_intensity_percent: 0,
            friendly_opacity_percent: 10,
            reduced_motion: true,
            high_contrast_telegraphs: evidence_preset.as_deref() != Some("thick_outline"),
            hostile_theme: if evidence_preset.as_deref() == Some("thick_outline") {
                HostileTheme::ThickOutline
            } else {
                HostileTheme::ColorblindSafe
            },
        }
    } else {
        AccessibilitySettings::default()
    };
    app.insert_resource(settings)
        .insert_resource(AccessibilityEvidence(evidence))
        .add_systems(Startup, spawn_overlay)
        .add_systems(
            Update,
            (update_controls, apply_effect_settings, update_overlay)
                .chain()
                .in_set(FrameSet::Presentation),
        );
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn spawn_overlay(mut commands: Commands, evidence: Res<AccessibilityEvidence>) {
    commands.spawn((
        Name::new("Accessibility controls"),
        AccessibilityOverlay,
        Text::new(""),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb_u8(235, 239, 226)),
        Node {
            position_type: PositionType::Absolute,
            top: px(104),
            right: px(14),
            width: px(360),
            border: UiRect::all(px(2)),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(5, 9, 13, 240)),
        BorderColor::all(Color::srgba_u8(238, 226, 150, 230)),
        if evidence.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        },
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn update_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<AccessibilitySettings>,
    mut overlay: Single<&mut Visibility, With<AccessibilityOverlay>>,
) {
    if keyboard.just_pressed(OVERLAY_TOGGLE) {
        **overlay = match **overlay {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
    if **overlay == Visibility::Hidden {
        return;
    }
    if keyboard.just_pressed(KeyCode::Digit1) {
        settings.high_contrast_telegraphs = !settings.high_contrast_telegraphs;
    }
    if keyboard.just_pressed(KeyCode::Digit2) {
        settings.reduced_motion = !settings.reduced_motion;
    }
    if keyboard.just_pressed(KeyCode::Digit3) {
        let next = cycle_percent(settings.screen_shake_percent, 0, 50, 100);
        settings.set_screen_shake(next).expect("preset is valid");
    }
    if keyboard.just_pressed(KeyCode::Digit4) {
        let next = cycle_percent(settings.flash_intensity_percent, 0, 50, 100);
        settings.set_flash_intensity(next).expect("preset is valid");
    }
    if keyboard.just_pressed(KeyCode::Digit5) {
        let next = cycle_percent(settings.friendly_opacity_percent, 10, 35, 60);
        settings
            .set_friendly_opacity(next)
            .expect("preset is valid");
    }
    if keyboard.just_pressed(KeyCode::Digit6) {
        settings.hostile_theme = settings.hostile_theme.next();
    }
}

const fn cycle_percent(current: u8, low: u8, middle: u8, high: u8) -> u8 {
    if current < middle {
        middle
    } else if current < high {
        high
    } else {
        low
    }
}

#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
fn apply_effect_settings(
    settings: Res<AccessibilitySettings>,
    mut friendly: Query<
        &mut Sprite,
        (
            With<ProjectilePresentation>,
            Without<HostileProjectilePresentation>,
        ),
    >,
    mut hostile: Query<
        (&mut Sprite, &HostileOutlineBaseSize),
        (
            With<HostileProjectilePresentation>,
            Without<ProjectilePresentation>,
        ),
    >,
) {
    if !settings.is_changed() {
        return;
    }
    let friendly_alpha = f32::from(settings.friendly_opacity_percent) / 100.0;
    for mut sprite in &mut friendly {
        sprite.color = sprite.color.with_alpha(friendly_alpha);
    }
    let hostile_outline = match (settings.high_contrast_telegraphs, settings.hostile_theme) {
        (true, _) => Color::srgb_u8(255, 255, 255),
        (false, HostileTheme::ShapeAndColor) => Color::srgb_u8(244, 239, 214),
        (false, HostileTheme::ColorblindSafe) => Color::srgb_u8(100, 225, 255),
        (false, HostileTheme::ThickOutline) => Color::srgb_u8(255, 224, 118),
    };
    for (mut sprite, base_size) in &mut hostile {
        sprite.color = hostile_outline;
        sprite.custom_size = Some(if settings.hostile_theme == HostileTheme::ThickOutline {
            base_size.0 * 1.35
        } else {
            base_size.0
        });
    }
    debug_assert!(AccessibilitySettings::hostile_mechanics_visible());
}

#[allow(clippy::needless_pass_by_value)]
fn update_overlay(
    settings: Res<AccessibilitySettings>,
    mut overlay: Single<&mut Text, With<AccessibilityOverlay>>,
) {
    if !settings.is_changed() {
        return;
    }
    overlay.0 = format!(
        "ACCESSIBILITY [F6]\n[1] HIGH CONTRAST  {}\n[2] REDUCED MOTION {}\n[3] SCREEN SHAKE   {}%\n[4] FLASH          {}% (LOCAL ONLY)\n[5] FRIENDLY FX    {}%\n[6] HOSTILE THEME  {}\n\nSHAPE + COLOR | HOSTILE TELEGRAPHS NEVER CULLED",
        on_off(settings.high_contrast_telegraphs),
        on_off(settings.reduced_motion),
        settings.screen_shake_percent,
        settings.flash_intensity_percent,
        settings.friendly_opacity_percent,
        settings.hostile_theme.label(),
    );
}

const fn on_off(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_boundaries_match_the_first_playable_contract() {
        let mut settings = AccessibilitySettings::default();
        assert_eq!(settings.screen_shake_percent, 50);
        assert_eq!(settings.flash_intensity_percent, 50);
        assert_eq!(settings.friendly_opacity_percent, 35);
        assert!(AccessibilitySettings::hostile_mechanics_visible());

        assert!(settings.set_screen_shake(100).is_ok());
        assert!(settings.set_screen_shake(101).is_err());
        assert!(settings.set_flash_intensity(0).is_ok());
        assert!(settings.set_flash_intensity(101).is_err());
        assert!(settings.set_friendly_opacity(10).is_ok());
        assert!(settings.set_friendly_opacity(60).is_ok());
        assert!(settings.set_friendly_opacity(9).is_err());
        assert!(settings.set_friendly_opacity(61).is_err());
    }

    #[test]
    fn presets_cycle_without_an_invalid_intermediate_value() {
        assert_eq!(cycle_percent(0, 0, 50, 100), 50);
        assert_eq!(cycle_percent(50, 0, 50, 100), 100);
        assert_eq!(cycle_percent(100, 0, 50, 100), 0);
        assert_eq!(cycle_percent(10, 10, 35, 60), 35);
        assert_eq!(cycle_percent(35, 10, 35, 60), 60);
        assert_eq!(cycle_percent(60, 10, 35, 60), 10);
    }
}
