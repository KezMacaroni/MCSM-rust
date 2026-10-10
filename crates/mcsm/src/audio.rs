//! Main-menu music, read from the user's FMOD banks.
//!
//! The menu's music agent (`ui_bgMusic_menuMain.scene`) plays the FMOD event
//! `mus_ivorLight_MainMenu2` (name from the game's `Master Bank.strings.bank`). Its audio is the
//! `Ivor_090_16BARS_ABCMenuMix` stream in `Common.bank`. FMOD event logic (sections, transitions)
//! is not decoded, so the stream is looped as-is at the agent's "Sound Music - Volume".
//!
//! Banks are RIFF files whose `SND ` chunk holds an FSB5 container; the `fsbex` crate (MIT/Apache)
//! turns its Vorbis streams into standard Ogg Vorbis, which Bevy plays directly.

use crate::data::GameData;
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;
use std::io::Cursor;

const MUSIC_BANK: &str = "Common.bank";
const MENU_STREAM: &str = "Ivor_090_16BARS_ABCMenuMix";
const MUSIC_SCENE: &str = "ui_bgMusic_menuMain.scene";
const MUSIC_AGENT: &str = "audio_music_interface_ui_bgMusic_menuMain";

/// Extract one stream (matched by its name before the " {guid}" suffix) as Ogg Vorbis.
pub fn bank_stream_ogg(bank: &[u8], stream: &str) -> Result<Vec<u8>, String> {
    let fsb = bank.windows(4).position(|w| w == b"FSB5").ok_or("no FSB5 data in bank")?;
    let parsed = fsbex::Bank::new(Cursor::new(&bank[fsb..])).map_err(|e| e.to_string())?;
    for s in parsed {
        let name = s.name().unwrap_or("");
        if name.split(" {").next() == Some(stream) {
            let mut out = s.write(Vec::new()).map_err(|e| e.to_string())?;
            fix_end_of_stream_flags(&mut out)?;
            return Ok(out);
        }
    }
    Err(format!("stream {stream} not found"))
}

/// fsbex 0.3 marks every Ogg page after the headers as end-of-stream, so decoders stop after
/// the first audio page and play silence. Keep the flag only on the last page and recompute
/// each page's CRC.
fn fix_end_of_stream_flags(ogg: &mut [u8]) -> Result<(), String> {
    const EOS: u8 = 0x04;
    let mut pages = Vec::new();
    let mut o = 0;
    while o < ogg.len() {
        if ogg.get(o..o + 4) != Some(b"OggS") || o + 27 > ogg.len() {
            return Err(format!("malformed Ogg page at byte {o}"));
        }
        let segments = ogg[o + 26] as usize;
        let body: usize = ogg.get(o + 27..o + 27 + segments).ok_or("truncated Ogg page")?.iter().map(|&b| b as usize).sum();
        let end = o + 27 + segments + body;
        if end > ogg.len() {
            return Err("truncated Ogg page".into());
        }
        pages.push((o, end));
        o = end;
    }
    let last = pages.len().saturating_sub(1);
    for (i, &(start, end)) in pages.iter().enumerate() {
        let flags = &mut ogg[start + 5];
        let want = if i == last { *flags | EOS } else { *flags & !EOS };
        if want != *flags {
            *flags = want;
            ogg[start + 22..start + 26].fill(0);
            let crc = ogg_crc(&ogg[start..end]);
            ogg[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
        }
    }
    Ok(())
}

/// Ogg page checksum: CRC-32, polynomial 0x04C11DB7, no reflection, zero init and final xor.
fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in data {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04C1_1DB7 } else { crc << 1 };
        }
    }
    crc
}

fn music_volume(data: &GameData, props: &mut crate::data::Props) -> f32 {
    let Some(scene) = data.read(MUSIC_SCENE).and_then(|b| ttmeta::Scene::parse(&b).ok()) else { return 1.0 };
    scene
        .agents
        .iter()
        .find(|a| a.name == MUSIC_AGENT)
        .and_then(|a| props.float(data, &a.props, "Sound Music - Volume"))
        .unwrap_or(1.0)
}

pub fn start_menu_music(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    mut props: ResMut<crate::data::Props>,
    mut sources: ResMut<Assets<AudioSource>>,
) {
    let Some(data) = data else { return };
    let Some(bank) = data.read(MUSIC_BANK) else {
        error!("{MUSIC_BANK} missing; no menu music");
        return;
    };
    match bank_stream_ogg(&bank, MENU_STREAM) {
        Ok(ogg) => {
            if let Some(path) = std::env::var_os("MCSM_DUMP_MUSIC") {
                let _ = std::fs::write(path, &ogg);
            }
            let volume = music_volume(&data, &mut props);
            info!("menu music: {MENU_STREAM} ({} KiB ogg), volume {volume}", ogg.len() / 1024);
            let source = sources.add(AudioSource { bytes: ogg.into() });
            commands.spawn((
                Name::new("menu music"),
                crate::settings::MusicVolume(volume),
                AudioPlayer(source),
                PlaybackSettings::LOOP.with_volume(Volume::Linear(volume)),
            ));
        }
        Err(e) => error!("menu music: {e}"),
    }
}

const SFX_BANK: &str = "Menu.bank";

/// Menu sound effects (the streams the menu's sfx chores trigger: `sfx_select`, `sfx_press`,
/// `sfx_sliderIncrease` / `Decrease`, `sfx_back`).
#[derive(Resource)]
pub struct MenuSfx {
    pub select: Handle<AudioSource>,
    pub press: Handle<AudioSource>,
    pub slider_increase: Option<Handle<AudioSource>>,
    pub slider_decrease: Option<Handle<AudioSource>>,
    pub back: Option<Handle<AudioSource>>,
    /// "Volume Master".
    pub volume: f32,
}

impl MenuSfx {
    pub fn play(&self, commands: &mut Commands, sound: &Handle<AudioSource>) {
        commands.spawn((AudioPlayer(sound.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(self.volume))));
    }

    pub fn play_opt(&self, commands: &mut Commands, sound: &Option<Handle<AudioSource>>) {
        if let Some(s) = sound {
            self.play(commands, s);
        }
    }
}

pub fn load_menu_sfx(mut commands: Commands, data: Option<Res<GameData>>, mut sources: ResMut<Assets<AudioSource>>) {
    let Some(data) = data else { return };
    let Some(bank) = data.read(SFX_BANK) else {
        error!("{SFX_BANK} missing; no menu sounds");
        return;
    };
    let mut load = |name: &str| match bank_stream_ogg(&bank, name) {
        Ok(ogg) => Some(sources.add(AudioSource { bytes: ogg.into() })),
        Err(e) => {
            error!("menu sound {name}: {e}");
            None
        }
    };
    if let (Some(select), Some(press)) = (load("ui_select"), load("ui_press")) {
        let (slider_increase, slider_decrease, back) = (load("ui_sliderIncrease"), load("ui_sliderDecrease"), load("ui_back"));
        commands.insert_resource(MenuSfx { select, press, slider_increase, slider_decrease, back, volume: 1.0 });
    }
}
