//! Player settings: the preference keys the game's settings menus edit, with the game's own
//! defaults, saved to our own file and applied to the running app.
//!
//! Behaviour follows the game's `UI_Setting.lua` (read for reference, not copied): widgets
//! stage changes with `Settings_Set`; sliders are "immediate" (applied while dragging);
//! leaving the settings menu with staged changes asks to Apply (commit and save) or Restore
//! (revert). Defaults come from the game's `project_*.prop` files. The user's own game
//! preferences are never written; ours go to `%APPDATA%\mcsm-rs\settings.txt`.

use crate::data::{GameData, Props};
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Float(f32),
    Size(u32, u32),
}

impl SettingValue {
    fn encode(self) -> String {
        match self {
            SettingValue::Bool(b) => b.to_string(),
            SettingValue::Float(f) => f.to_string(),
            SettingValue::Size(w, h) => format!("{w}x{h}"),
        }
    }

    fn decode(s: &str, like: SettingValue) -> Option<SettingValue> {
        let s = s.trim();
        Some(match like {
            SettingValue::Bool(_) => SettingValue::Bool(s.parse().ok()?),
            SettingValue::Float(_) => SettingValue::Float(s.parse().ok()?),
            SettingValue::Size(..) => {
                let (w, h) = s.split_once('x')?;
                SettingValue::Size(w.parse().ok()?, h.parse().ok()?)
            }
        })
    }
}

pub const VOLUME_MASTER: &str = "Volume Master";
pub const VOLUME_MUSIC: &str = "Volume Music";
pub const SUBTITLES: &str = "Enable Subtitles";
pub const SUBTITLE_SIZE: &str = "Subtitle Size";
pub const BRIGHTNESS: &str = "Brightness";
pub const INVERT_Y: &str = "Invert Joystick";
pub const RETICLE_DOTS: &str = "Enable Reticle Hint Dots";
pub const TEXTURE_QUALITY: &str = "Texture Quality";
pub const ANTI_ALIASING: &str = "FX anti-aliasing";
pub const WINDOWED: &str = "Windowed";
pub const WINDOW_SIZE: &str = "Window Size";

/// (key, the game's prop file holding its default, fallback default).
const KEYS: [(&str, &str, SettingValue); 11] = [
    (VOLUME_MASTER, "project_audio.prop", SettingValue::Float(1.0)),
    (VOLUME_MUSIC, "project_audio.prop", SettingValue::Float(1.0)),
    (SUBTITLES, "project_general.prop", SettingValue::Bool(true)),
    (SUBTITLE_SIZE, "project_settings.prop", SettingValue::Float(0.5)),
    (BRIGHTNESS, "project_settings.prop", SettingValue::Float(0.5)),
    (INVERT_Y, "project_settings.prop", SettingValue::Bool(false)),
    (RETICLE_DOTS, "project_settings.prop", SettingValue::Bool(true)),
    (TEXTURE_QUALITY, "project_gamewindow.prop", SettingValue::Float(0.0)),
    (ANTI_ALIASING, "project_gamewindow.prop", SettingValue::Bool(true)),
    // This app starts windowed (switching the screen mode unasked would be rude).
    (WINDOWED, "", SettingValue::Bool(true)),
    (WINDOW_SIZE, "", SettingValue::Size(1280, 720)),
];

/// Keys applied as soon as they change (the game's sliders); the rest wait for Apply.
fn immediate(key: &str) -> bool {
    matches!(key, VOLUME_MASTER | VOLUME_MUSIC | BRIGHTNESS)
}

#[derive(Resource)]
pub struct Settings {
    committed: HashMap<&'static str, SettingValue>,
    /// Staged values (Settings_Set), and each key's value before staging.
    staged: HashMap<&'static str, SettingValue>,
    path: Option<PathBuf>,
}

fn settings_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("mcsm-rs").join("settings.txt"))
}

impl Settings {
    fn static_key(key: &str) -> Option<&'static str> {
        KEYS.iter().map(|k| k.0).find(|k| *k == key)
    }

    /// The value the menus show: staged if any, else committed.
    pub fn get(&self, key: &str) -> SettingValue {
        let k = Self::static_key(key).expect("unknown setting");
        self.staged.get(k).or_else(|| self.committed.get(k)).copied().unwrap()
    }

    /// The value in effect: staged for immediate keys, committed otherwise.
    pub fn applied(&self, key: &str) -> SettingValue {
        let k = Self::static_key(key).expect("unknown setting");
        if immediate(k) {
            self.get(k)
        } else {
            self.committed[k]
        }
    }

    pub fn set(&mut self, key: &str, value: SettingValue) {
        let k = Self::static_key(key).expect("unknown setting");
        self.staged.insert(k, value);
    }

    pub fn is_dirty(&self) -> bool {
        self.staged.iter().any(|(k, v)| self.committed.get(k) != Some(v))
    }

    pub fn commit(&mut self) {
        for (k, v) in self.staged.drain() {
            self.committed.insert(k, v);
        }
        self.save();
    }

    pub fn revert(&mut self) {
        self.staged.clear();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let mut text = String::from("# mcsm-rs settings (the game's preference keys)\n");
        for (k, _, _) in KEYS {
            text += &format!("{k} = {}\n", self.committed[k].encode());
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, text) {
            warn!("could not save settings to {}: {e}", path.display());
        }
    }
}

pub fn load(mut commands: Commands, data: Option<Res<GameData>>, mut props: ResMut<Props>) {
    let mut committed = HashMap::new();
    for (key, file, fallback) in KEYS {
        let default = data
            .as_deref()
            .filter(|_| !file.is_empty())
            .and_then(|d| props.file(d, ttmeta::Symbol::of(file)).map(|set| (d, set)))
            .and_then(|(d, set)| match fallback {
                SettingValue::Bool(_) => props.bool(d, &set, key).map(SettingValue::Bool),
                SettingValue::Float(_) => props
                    .float(d, &set, key)
                    .or_else(|| props.int(d, &set, key).map(|i| i as f32))
                    .map(SettingValue::Float),
                SettingValue::Size(..) => None,
            })
            .unwrap_or(fallback);
        committed.insert(key, default);
    }
    let path = settings_path();
    if let Some(text) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Some(key) = Settings::static_key(k.trim()) else { continue };
            if let Some(v) = SettingValue::decode(v, committed[key]) {
                committed.insert(key, v);
            }
        }
    }
    commands.insert_resource(Settings { committed, staged: HashMap::new(), path });
}

/// Music volume before settings (the music agent's own "Sound Music - Volume").
#[derive(Component)]
pub struct MusicVolume(pub f32);

/// The key-art camera's exposure before the brightness setting.
#[derive(Component)]
pub struct BaseExposure(pub f32);

/// Push the settings in effect into the app: volumes every frame (sinks appear late), the rest
/// when they change.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    mut music: Query<(&MusicVolume, &mut bevy::audio::AudioSink)>,
    mut sfx: Option<ResMut<crate::audio::MenuSfx>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cameras: Query<(Entity, &Camera, &mut bevy::camera::Exposure, Option<&BaseExposure>)>,
    mut last: Local<Option<(SettingValue, SettingValue, SettingValue)>>,
) {
    let Some(settings) = settings else { return };
    let master = settings.applied(VOLUME_MASTER);
    let music_level = settings.applied(VOLUME_MUSIC);
    let (SettingValue::Float(master), SettingValue::Float(music_level)) = (master, music_level) else { return };
    for (base, mut sink) in &mut music {
        let want = base.0 * master * music_level;
        if (sink.volume().to_linear() - want).abs() > 1e-4 {
            sink.set_volume(bevy::audio::Volume::Linear(want));
        }
    }
    if let Some(sfx) = sfx.as_mut() {
        if sfx.volume != master {
            sfx.volume = master;
        }
    }
    // Brightness: +-1 stop around the default on the 3D (key-art) camera.
    if let SettingValue::Float(b) = settings.applied(BRIGHTNESS) {
        for (e, cam, mut exposure, base) in &mut cameras {
            if cam.order != 0 {
                continue;
            }
            let base = match base {
                Some(b) => b.0,
                None => {
                    commands.entity(e).insert(BaseExposure(exposure.ev100));
                    exposure.ev100
                }
            };
            let want = base - (b - 0.5) * 2.0;
            if (exposure.ev100 - want).abs() > 1e-4 {
                exposure.ev100 = want;
            }
        }
    }

    let now = (settings.applied(WINDOWED), settings.applied(WINDOW_SIZE), settings.applied(ANTI_ALIASING));
    if last.as_ref() == Some(&now) {
        return;
    }
    *last = Some(now);
    if let Ok(mut window) = windows.single_mut() {
        match now.0 {
            SettingValue::Bool(false) => window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => {
                window.mode = WindowMode::Windowed;
                if let SettingValue::Size(w, h) = now.1 {
                    window.resolution.set(w as f32, h as f32);
                }
            }
        }
    }
    let fxaa = now.2 == SettingValue::Bool(true);
    for (e, cam, _, _) in &cameras {
        if cam.order == 0 {
            if fxaa {
                commands.entity(e).insert(bevy::anti_alias::fxaa::Fxaa::default());
            } else {
                commands.entity(e).remove::<bevy::anti_alias::fxaa::Fxaa>();
            }
        }
    }
}

/// Screen sizes offered by the resolution slider: the monitors' video modes of at least
/// 800x600 (as the game's `getResolutions`), plus the current size.
pub fn resolutions(monitors: &[(u32, u32)], current: (u32, u32)) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = monitors.iter().copied().filter(|&(w, h)| w >= 800 && h >= 600).collect();
    v.push(current);
    v.sort();
    v.dedup();
    v
}
