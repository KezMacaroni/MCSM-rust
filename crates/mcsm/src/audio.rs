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

/// Episode voices are single-stream FSB5 MPEG banks despite the `.wav` extension.
/// FSB aligns each MP3 frame to four bytes; remove that padding for Bevy's MP3 decoder.
pub fn voice_stream(bank: &[u8], id: u32) -> Result<Vec<u8>, String> {
    let u32_at = |at| bank.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    if bank.get(..4) != Some(b"FSB5") { return Err("voice is not FSB5".into()); }
    if u32_at(24) != Some(11) { return bank_stream_ogg(bank, &id.to_string()); }
    if u32_at(8) != Some(1) { return Err("multi-stream MPEG voice not supported".into()); }
    let header: usize = match u32_at(4) { Some(0) => 64, Some(1) => 60, _ => return Err("unknown FSB version".into()) };
    let start = header.checked_add(u32_at(12).ok_or("short FSB header")? as usize)
        .and_then(|s| s.checked_add(u32_at(16)? as usize)).ok_or("invalid FSB sizes")?;
    let end = start.checked_add(u32_at(20).ok_or("short FSB header")? as usize).ok_or("invalid FSB data length")?;
    let bytes = bank.get(start..end).ok_or("truncated MPEG voice")?;
    unpad_mp3(bytes)
}

fn unpad_mp3(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= bytes.len() {
        if bytes[at..].iter().all(|b| *b == 0) { break; }
        let h = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let version = (h >> 19) & 3;
        let bitrate = ((h >> 12) & 15) as usize;
        let rate = ((h >> 10) & 3) as usize;
        if h >> 21 != 0x7ff || (h >> 17) & 3 != 1 || version == 1 || bitrate == 0 || bitrate == 15 || rate == 3 {
            return Err(format!("invalid MPEG Layer III frame at {at}"));
        }
        let kbps = if version == 3 { [0,32,40,48,56,64,80,96,112,128,160,192,224,256,320,0][bitrate] }
            else { [0,8,16,24,32,40,48,56,64,80,96,112,128,144,160,0][bitrate] };
        let hz = [44100,48000,32000][rate] / if version == 3 { 1 } else if version == 2 { 2 } else { 4 };
        let size = (if version == 3 { 144 } else { 72 }) * kbps * 1000 / hz + ((h >> 9) & 1) as usize;
        out.extend_from_slice(bytes.get(at..at + size).ok_or("truncated MP3 frame")?);
        at += (size + 3) & !3;
    }
    if out.is_empty() { return Err("empty MPEG voice".into()); }
    Ok(out)
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

#[derive(Component)]
pub struct MenuMusic;

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
                MenuMusic,
                crate::settings::MusicVolume(volume),
                AudioPlayer(source),
                PlaybackSettings::LOOP.with_volume(Volume::Linear(volume)),
            ));
        }
        Err(e) => error!("menu music: {e}"),
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;
    #[test]
    fn removes_fmod_frame_padding() {
        // MPEG1 Layer III, 48 kb/s, 44.1 kHz, padding bit set: 157 bytes.
        let mut frame = vec![0; 160];
        frame[..4].copy_from_slice(&[0xff, 0xfb, 0x32, 0xc0]);
        let bytes = [frame.clone(), frame].concat();
        let mp3 = unpad_mp3(&bytes).unwrap();
        assert_eq!(mp3.len(), 314);
        assert_eq!(&mp3[157..161], &[0xff, 0xfb, 0x32, 0xc0]);
    }
    #[test]
    fn rejects_bad_or_truncated_frames() {
        assert!(unpad_mp3(&[1, 2, 3, 4]).is_err());
        assert!(unpad_mp3(&[0xff, 0xfb, 0x32, 0xc0]).is_err());
        assert!(voice_stream(b"FSB5", 1).is_err());
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
