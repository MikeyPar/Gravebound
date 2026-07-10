use bevy::{prelude::*, sprite::Text2dShadow, window::WindowResolution};

const WINDOW_TITLE: &str = "Gravebound — LocalLab";

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb_u8(9, 12, 15)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: WINDOW_TITLE.to_owned(),
                resolution: WindowResolution::new(1280, 720),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup_foundation_screen)
        .run();
}

fn setup_foundation_screen(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text2d::new(format!(
            "GRAVEBOUND\nLOCAL LAB FOUNDATION\n{} HZ AUTHORITATIVE SIMULATION",
            sim_core::TICKS_PER_SECOND
        )),
        TextColor(Color::srgb_u8(218, 184, 112)),
        TextFont::from_font_size(34.0),
        Text2dShadow::default(),
    ));
}
