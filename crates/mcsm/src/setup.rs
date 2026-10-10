//! First-run setup: when no working install is configured, ask for the game folder with the
//! system folder picker and remember the answer in the per-user config.

use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::{Path, PathBuf};
use ttarch::install::{save_game_dir, EXE_NAME};
use ttarch::Install;

const TITLE: &str = "Minecraft: Story Mode (mcsm-rs)";

/// Usual install folders, used as the picker's starting point when one exists.
const LIKELY_PARENTS: &[&str] = &[
    r"C:\Program Files (x86)\Steam\steamapps\common",
    r"C:\Program Files\Steam\steamapps\common",
    r"C:\Program Files (x86)",
    r"C:\Program Files",
];

/// Ask for the game folder until one opens or the player cancels. `None` on cancel.
pub fn ask_for_install() -> Option<Install> {
    let start = LIKELY_PARENTS.iter().map(Path::new).find(|p| p.is_dir());
    loop {
        let mut dialog = FileDialog::new().set_title(format!("Choose the folder that contains {EXE_NAME}"));
        if let Some(start) = start {
            dialog = dialog.set_directory(start);
        }
        let picked = dialog.pick_folder()?;
        let found = find_exe(&picked);
        match found.as_deref().map(Install::open) {
            Some(Ok(install)) => {
                match save_game_dir(&install.root) {
                    Ok(path) => bevy::log::info!("saved game folder to {}", path.display()),
                    Err(e) => bevy::log::warn!("could not save the game folder: {e}"),
                }
                return Some(install);
            }
            Some(Err(e)) => retry(&format!("{} could not be opened:\n\n{e}", picked.display()))?,
            None => retry(&format!(
                "{EXE_NAME} was not found in\n{}\n\nChoose the Minecraft: Story Mode folder (Season 1, PC).",
                picked.display()
            ))?,
        }
    }
}

/// Show `message` with Retry / Cancel; `None` on cancel.
fn retry(message: &str) -> Option<()> {
    let answer = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title(TITLE)
        .set_description(message)
        .set_buttons(MessageButtons::OkCancelCustom("Choose again".into(), "Cancel".into()))
        .show();
    matches!(answer, MessageDialogResult::Custom(ref s) if s == "Choose again").then_some(())
}

/// The game exe in `dir` or up to two folders below it, so picking e.g. Steam's `common`
/// folder still works.
fn find_exe(dir: &Path) -> Option<PathBuf> {
    fn search(dir: &Path, depth: u32) -> Option<PathBuf> {
        let exe = dir.join(EXE_NAME);
        if exe.is_file() {
            return Some(exe);
        }
        if depth == 0 {
            return None;
        }
        let mut subdirs: Vec<PathBuf> =
            std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect();
        subdirs.sort();
        subdirs.iter().find_map(|d| search(d, depth - 1))
    }
    search(dir, 2)
}
