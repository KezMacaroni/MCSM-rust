//! Parse every file of each supported kind in the user's install (skipped without MCSM_GAME_DIR).
//! Reports failures by name; asserts structure only, never content.

use ttarch::install::Vfs;
use ttarch::Install;

fn vfs() -> Option<Vfs> {
    let dir = std::env::var_os("MCSM_GAME_DIR")?;
    Some(Install::open(dir.as_ref()).unwrap().mount().unwrap())
}

/// Run `parse` over every (deduplicated) entry with `ext`; return (ok, failures).
fn sweep(vfs: &Vfs, ext: &str, parse: impl Fn(&[u8]) -> Result<(), String>) -> (usize, Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    let (mut ok, mut bad) = (0, Vec::new());
    for a in &vfs.archives {
        for e in a.entries().iter().filter(|e| e.name.to_ascii_lowercase().ends_with(ext)) {
            if !seen.insert(e.name.to_ascii_lowercase()) {
                continue;
            }
            match parse(&a.read(e).unwrap()) {
                Ok(()) => ok += 1,
                Err(err) => bad.push(format!("{}: {err}", e.name)),
            }
        }
    }
    (ok, bad)
}

fn report(kind: &str, (ok, bad): (usize, Vec<String>)) -> (usize, usize) {
    eprintln!("{kind}: {ok} ok, {} failed", bad.len());
    for b in bad.iter().take(8) {
        eprintln!("   {b}");
    }
    (ok, bad.len())
}

#[test]
fn textures() {
    let Some(vfs) = vfs() else { return };
    let (ok, bad) = report(".d3dtx", sweep(&vfs, ".d3dtx", |b| ttmeta::Texture::parse(b).map(|_| ()).map_err(|e| e.to_string())));
    assert!(ok > 0 && bad == 0);
}

#[test]
fn meshes() {
    let Some(vfs) = vfs() else { return };
    let (ok, bad) = report(".d3dmesh", sweep(&vfs, ".d3dmesh", |b| ttmeta::Mesh::parse(b).map(|_| ()).map_err(|e| e.to_string())));
    assert!(ok > 0 && bad == 0);
}

#[test]
fn scenes_and_props() {
    let Some(vfs) = vfs() else { return };
    let (ok, bad) = report(".scene", sweep(&vfs, ".scene", |b| ttmeta::Scene::parse(b).map(|_| ()).map_err(|e| e.to_string())));
    assert!(ok > 0 && bad == 0);
    let (ok, bad) = report(".prop", sweep(&vfs, ".prop", |b| ttmeta::PropertySet::from_file(b).map(|_| ()).map_err(|e| e.to_string())));
    assert!(ok > 0 && bad == 0);
}

#[test]
fn fonts() {
    let Some(vfs) = vfs() else { return };
    let (ok, bad) = report(".font", sweep(&vfs, ".font", |b| ttmeta::Font::parse(b).map(|_| ()).map_err(|e| e.to_string())));
    assert!(ok > 0 && bad == 0);
}

#[test]
fn chores() {
    let Some(vfs) = vfs() else { return };
    // Chores that embed non-animation objects (procedural look-ats and similar) are counted
    // separately: they are recognised, just not decoded yet.
    let unsupported = std::sync::atomic::AtomicUsize::new(0);
    let (ok, bad) = report(
        ".chore",
        sweep(&vfs, ".chore", |b| match ttmeta::Chore::parse(b) {
            Ok(_) => Ok(()),
            Err(ttmeta::Error::Unsupported(_)) => {
                unsupported.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }),
    );
    eprintln!("   with unsupported embedded objects: {}", unsupported.into_inner());
    assert!(ok > 0 && bad == 0);
}

#[test]
fn animations() {
    let Some(vfs) = vfs() else { return };
    // Every .anm must parse structurally; keyframed values and skeleton keys are decoded.
    let keyframed = std::sync::atomic::AtomicUsize::new(0);
    let skeletal = std::sync::atomic::AtomicUsize::new(0);
    let (ok, bad) = report(
        ".anm",
        sweep(&vfs, ".anm", |b| {
            let a = ttmeta::Animation::parse(b).map_err(|e| e.to_string())?;
            if a.complete {
                keyframed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(k) = a.skeleton() {
                skeletal.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                for q in k.rotations.iter().flatten() {
                    let len = q.iter().map(|c| c * c).sum::<f32>().sqrt();
                    if !(0.9..1.1).contains(&len) {
                        return Err(format!("rotation of length {len}"));
                    }
                }
            }
            Ok(())
        }),
    );
    eprintln!("   fully decoded: {}, with skeleton keys: {}", keyframed.into_inner(), skeletal.into_inner());
    assert!(ok > 0 && bad == 0);
}

#[test]
fn skeletons() {
    let Some(vfs) = vfs() else { return };
    let (ok, bad) = report(".skl", sweep(&vfs, ".skl", |b| ttmeta::Skeleton::parse(b).map(|_| ()).map_err(|e| e.to_string())));
    assert!(ok > 0 && bad == 0);
}
