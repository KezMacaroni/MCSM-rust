//! Minecraft: Story Mode on Bevy, running from the user's own install.
//!
//!   cargo run -p mcsm --release -- [path to the game folder or MinecraftStoryMode.exe]
//! or set MCSM_GAME_DIR, or put `game_dir = "..."` in ./mcsm.toml. Otherwise it asks for the
//! game folder on startup and remembers it in %APPDATA%\mcsm-rs\mcsm.toml.
//!
//! Set MCSM_SCREENSHOT=<file.png> to save a screenshot after the menu has settled and exit
//! (used for automated visual checks).

mod audio;
mod character;
mod cursor;
mod chore;
mod convert;
mod data;
mod lit;
mod grade;
mod menu;
mod raw;
mod settings;
mod setup;
mod source;
mod stage;
mod story;
mod text;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::path::PathBuf;
use std::sync::Arc;
use ttarch::Install;

#[derive(Resource)]
struct BootError(String);

fn main() -> AppExit {
    let explicit = std::env::args().nth(1).map(PathBuf::from);
    let mut app = App::new();
    app.init_resource::<data::Props>().init_resource::<data::AssetCache>().init_resource::<menu::MenuState>();

    // No working install configured (and none given on the command line): ask for the folder.
    let located = Install::locate(explicit.as_deref())
        .or_else(|e| if explicit.is_none() { setup::ask_for_install().ok_or(e) } else { Err(e) });
    match located.and_then(|i| Ok((i.mount()?, i))) {
        Ok((vfs, install)) => {
            info!("install {}: {} archives, {} files", install.root.display(), vfs.archives.len(), vfs.file_count());
            for e in &vfs.errors {
                warn!("archive failed to mount: {e}");
            }
            let vfs = Arc::new(vfs);
            source::register(&mut app, vfs.clone());
            app.insert_resource(data::GameData::new(vfs));
        }
        Err(e) => {
            app.insert_resource(BootError(format!(
                "Could not open your Minecraft: Story Mode install.\n\n{e}\n\n\
                 Start mcsm.exe again and choose the folder that contains MinecraftStoryMode.exe\n\
                 (Minecraft: Story Mode, Season 1, PC)."
            )));
        }
    }

    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Minecraft: Story Mode (mcsm-rs)".into(), ..default() }),
        ..default()
    }))
    .add_plugins(raw::RawFilePlugin)
    .add_plugins(lit::LitPlugin)
    .add_plugins(grade::GradePlugin)
    .add_systems(PostUpdate, stage::grow_skinned_bounds.after(bevy::camera::visibility::VisibilitySystems::CalculateBounds).before(bevy::camera::visibility::VisibilitySystems::CheckVisibility))
    .add_systems(Startup, (boot_error, settings::load, cursor::load, menu::build, audio::start_menu_music, audio::load_menu_sfx).chain())
    .add_systems(Update, ((menu::follow_aspect, menu::episode_tick, menu::transition_tick, menu::prewarm, menu::rebuild, menu::sync_cameras, menu::input, menu::refresh, menu::apply_selection, menu::appearance, menu::episode_preview_controls, settings::apply, cursor::apply).chain(), (chore::play, character::apply_set_idle, character::animate, story::tick, stage::sync_lights, stage::apply_fov, debug_focus, fps_log, auto_screenshot).chain()).chain())
    .run()
}

fn boot_error(mut commands: Commands, error: Option<Res<BootError>>) {
    let Some(error) = error else { return };
    commands.spawn(Camera2d);
    commands.spawn((
        Text::new(error.0.clone()),
        TextFont { font_size: bevy::text::FontSize::Px(22.0), ..default() },
        Node { position_type: PositionType::Absolute, top: px(24), left: px(24), ..default() },
    ));
}

/// MCSM_SCREENSHOT=<file.png> [MCSM_SCREENSHOT_SECONDS=2,10,20]: save screenshots at those
/// times after startup (file names get the second appended), then exit.
/// MCSM_FPS_LOG: log frames per second once a second.
fn fps_log(time: Res<Time>, mut acc: Local<(f32, u32)>) {
    if std::env::var_os("MCSM_FPS_LOG").is_none() {
        return;
    }
    acc.0 += time.delta_secs();
    acc.1 += 1;
    if acc.0 >= 1.0 {
        info!("fps {:.1}", acc.1 as f32 / acc.0);
        *acc = (0.0, 0);
    }
}

fn auto_screenshot(mut commands: Commands, time: Res<Time>, mut taken: Local<usize>, mut exit: MessageWriter<AppExit>) {
    let Some(path) = std::env::var_os("MCSM_SCREENSHOT").map(PathBuf::from) else { return };
    let times: Vec<f32> = std::env::var("MCSM_SCREENSHOT_SECONDS")
        .ok()
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![3.0]);
    let now = time.elapsed_secs();
    if let Some(&at) = times.get(*taken) {
        if now >= at {
            let file = path.with_file_name(format!(
                "{}_{at}s.png",
                path.file_stem().unwrap_or_default().to_string_lossy()
            ));
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(file));
            *taken += 1;
        }
    } else if now >= times.last().copied().unwrap_or(0.0) + 1.5 {
        exit.write(AppExit::Success);
    }
}

/// MCSM_FOCUS=<agent>: debug view that points the key-art camera at an agent from 3 units in
/// front of it (agents face +Z) and hides the UI cameras.
fn debug_focus(
    names: Query<(&Name, &GlobalTransform)>,
    mut cams: Query<(&mut Camera, &ChildOf, &mut Transform)>,
    parents: Query<&GlobalTransform>,
) {
    let Ok(target) = std::env::var("MCSM_FOCUS") else { return };
    let Some((_, agent)) = names.iter().find(|(n, _)| n.as_str() == target) else { return };
    if std::env::var_os("MCSM_FOCUS_LOG").is_some() {
        let (sc, _, t) = agent.to_scale_rotation_translation();
        info!("focus {target}: at {t:?} scale {sc:?}");
    }
    let at = agent.translation() + Vec3::Y * 0.8;
    let dist: f32 = std::env::var("MCSM_FOCUS_DIST").ok().and_then(|d| d.parse().ok()).unwrap_or(3.0);
    let eye = at + agent.rotation() * Vec3::new(0.0, 0.3 + dist * 0.5, dist);
    if std::env::var_os("MCSM_FOCUS_LOG").is_some() {
        info!("focus {target}: at {at:?} eye {eye:?}");
    }
    for (mut cam, parent, mut tf) in &mut cams {
        if cam.order != 0 {
            cam.is_active = false;
            continue;
        }
        let Ok(pg) = parents.get(parent.parent()) else { continue };
        let world = Transform::from_translation(eye).looking_at(at, Vec3::Y);
        *tf = Transform::from_matrix(pg.to_matrix().inverse() * world.to_matrix());
    }
}

