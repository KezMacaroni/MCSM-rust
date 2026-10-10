//! Runs against the user's own install; skipped when MCSM_GAME_DIR is not set.
//! Asserts structure only, never content, so no game data ends up in test sources.

use ttarch::Install;

fn install() -> Option<Install> {
    let dir = std::env::var_os("MCSM_GAME_DIR")?;
    Some(Install::open(dir.as_ref()).expect("MCSM_GAME_DIR set but install did not open"))
}

#[test]
fn every_archive_mounts_and_every_entry_is_in_bounds() {
    let Some(install) = install() else {
        eprintln!("MCSM_GAME_DIR not set; skipping");
        return;
    };
    let vfs = install.mount().unwrap();
    for e in &vfs.errors {
        eprintln!("mount error: {e}");
    }
    assert!(vfs.errors.is_empty(), "{} archives failed to mount", vfs.errors.len());
    assert!(!vfs.archives.is_empty());
    for a in &vfs.archives {
        for e in a.entries() {
            assert!(!e.name.is_empty(), "{}: empty name", a.path().display());
        }
    }
}

#[test]
fn every_archived_script_decrypts_to_lua52_bytecode() {
    let Some(install) = install() else { return };
    let vfs = install.mount().unwrap();
    let (mut ok, mut bad) = (0, Vec::new());
    for a in &vfs.archives {
        for e in a.entries().iter().filter(|e| e.name.to_ascii_lowercase().ends_with(".lua")) {
            let plain = ttarch::script::decrypt(&install.cipher, &a.read(e).unwrap());
            if ttarch::script::is_lua52_bytecode(&plain) {
                ok += 1;
            } else {
                bad.push(e.name.clone());
            }
        }
    }
    assert!(bad.is_empty(), "{} of {} scripts did not decrypt: {:?}", bad.len(), ok + bad.len(), &bad[..bad.len().min(5)]);
    assert!(ok > 0);
}

#[test]
fn smallest_and_largest_entry_read_back_at_declared_size() {
    let Some(install) = install() else { return };
    let vfs = install.mount().unwrap();
    let all: Vec<_> = vfs.archives.iter().flat_map(|a| a.entries().iter().map(move |e| (a, e))).collect();
    let small = all.iter().min_by_key(|(_, e)| e.size).unwrap();
    let large = all.iter().max_by_key(|(_, e)| e.size).unwrap();
    for (a, e) in [small, large] {
        assert_eq!(a.read(e).unwrap().len(), e.size as usize, "{}", e.name);
    }
}
