//! Developer CLI for inspecting the user's own install.
//!
//!   mcsm-tools [--game <dir|exe>] info
//!   mcsm-tools [--game <dir|exe>] ls <archive-name-or-path> [filter]
//!   mcsm-tools [--game <dir|exe>] find <file-name>
//!   mcsm-tools [--game <dir|exe>] extract <file-name> <out-dir>
//!
//! `extract` refuses to write inside this repository: extracted game data must never be committed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use ttarch::Install;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut game = None;
    if let Some(i) = args.iter().position(|a| a == "--game") {
        game = Some(PathBuf::from(args.get(i + 1).ok_or("--game needs a path")?));
        args.drain(i..i + 2);
    }
    let install = Install::locate(game.as_deref())?;
    let cmd = args.first().map(String::as_str).unwrap_or("info");
    match cmd {
        "info" => info(&install),
        "ls" => ls(&install, args.get(1).ok_or("ls <archive>")?, args.get(2).map(String::as_str)),
        "find" => find(&install, args.get(1).ok_or("find <file-name>")?),
        "extract" => extract(
            &install,
            args.get(1).ok_or("extract <file-name> <out-dir>")?,
            Path::new(args.get(2).ok_or("extract <file-name> <out-dir>")?),
        ),
        "dump" => dump(
            &install,
            args.get(1).ok_or("dump <archive> <extension> <out-dir>")?,
            args.get(2).ok_or("dump <archive> <extension> <out-dir>")?,
            Path::new(args.get(3).ok_or("dump <archive> <extension> <out-dir>")?),
        ),
        "script" => script(
            &install,
            args.get(1).ok_or("script <file-name.lua> <out-dir>")?,
            Path::new(args.get(2).ok_or("script <file-name.lua> <out-dir>")?),
        ),
        "meta" => meta(&install, args.get(1).ok_or("meta <file-name>")?),
        other => Err(format!("unknown command {other:?} (info, ls, find, extract, dump, script, meta)").into()),
    }
}

fn info(install: &Install) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = install.mount()?;
    println!("install   {}", install.root.display());
    println!("archives  {} mounted, {} failed", vfs.archives.len(), vfs.errors.len());
    for e in &vfs.errors {
        println!("  failed: {e}");
    }
    println!("files     {}", vfs.file_count());
    let mut by_ext: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for a in &vfs.archives {
        for e in a.entries() {
            let ext = e.name.rsplit_once('.').map_or("(none)", |(_, x)| x).to_ascii_lowercase();
            let slot = by_ext.entry(ext).or_default();
            slot.0 += 1;
            slot.1 += u64::from(e.size);
        }
    }
    let mut rows: Vec<_> = by_ext.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    println!("\n{:>8} {:>12}  type", "count", "MiB");
    for (ext, (n, bytes)) in rows {
        println!("{n:>8} {:>12.1}  .{ext}", bytes as f64 / (1 << 20) as f64);
    }
    Ok(())
}

fn resolve_archive(install: &Install, name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let p = PathBuf::from(name);
    if p.is_file() {
        return Ok(p);
    }
    let want = name.to_ascii_lowercase();
    install
        .archive_paths()?
        .into_iter()
        .find(|p| {
            let f = p.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            f == want || f == format!("{want}.ttarch2") || f.contains(&want)
        })
        .ok_or_else(|| format!("no archive matching {name:?}").into())
}

fn ls(install: &Install, archive: &str, filter: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let a = install.open_archive(&resolve_archive(install, archive)?)?;
    let filter = filter.map(str::to_ascii_lowercase);
    for e in a.entries() {
        if filter.as_ref().is_none_or(|f| e.name.to_ascii_lowercase().contains(f)) {
            println!("{:>10}  {}", e.size, e.name);
        }
    }
    Ok(())
}

fn find(install: &Install, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = install.mount()?;
    let mut hits = 0;
    for a in &vfs.archives {
        if let Some(e) = a.find(name) {
            println!("{:>10}  {}", e.size, a.path().file_name().unwrap().to_string_lossy());
            hits += 1;
        }
    }
    if hits == 0 {
        return Err(format!("{name:?} not found in any archive").into());
    }
    Ok(())
}

/// Create `out` and make sure it is not inside this repository.
fn output_dir(out: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out)?;
    let out = out.canonicalize()?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    if out.starts_with(&repo) {
        return Err(format!("refusing to extract into the repository ({}); pick a folder outside it", repo.display()).into());
    }
    Ok(out)
}

fn dump(install: &Install, archive: &str, ext: &str, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let out = output_dir(out)?;
    let a = install.open_archive(&resolve_archive(install, archive)?)?;
    let suffix = format!(".{}", ext.trim_start_matches('.').to_ascii_lowercase());
    let mut n = 0;
    for e in a.entries().iter().filter(|e| e.name.to_ascii_lowercase().ends_with(&suffix)) {
        std::fs::write(out.join(Path::new(&e.name).file_name().unwrap()), a.read(e)?)?;
        n += 1;
    }
    println!("{n} files -> {}", out.display());
    Ok(())
}

/// Symbol names for display: every archived file name plus printable strings from the user's exe.
fn names(install: &Install, vfs: &ttarch::install::Vfs) -> ttmeta::Names {
    let mut n = ttmeta::Names::default();
    for a in &vfs.archives {
        for e in a.entries() {
            n.add(&e.name);
        }
    }
    if let Ok(exe) = std::fs::read(&install.exe) {
        for run in exe.split(|b| !(0x20..0x7f).contains(b)).filter(|r| r.len() >= 3 && r.len() < 120) {
            n.add(&String::from_utf8_lossy(run));
        }
    }
    n
}

fn show_props(n: &ttmeta::Names, p: &ttmeta::PropertySet, indent: &str) {
    let parents: Vec<_> = p.parents.iter().map(|s| n.show(*s)).collect();
    println!("{indent}parents {parents:?}");
    for (k, t, v) in &p.values {
        println!("{indent}{} = {v:?}  [{}]", n.show(*k), n.show(ttmeta::Symbol(*t)));
    }
    if let Some((k, t)) = p.stopped_at {
        println!("{indent}... stopped at {} [{}]", n.show(k), n.show(ttmeta::Symbol(t)));
    }
}

fn meta(install: &Install, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = install.mount()?;
    // A loose file (e.g. the game's saved prefs.prop) or an archived one.
    let bytes = if Path::new(name).is_file() {
        std::fs::read(name)?
    } else {
        let (a, e) = vfs.find(name).ok_or_else(|| format!("{name:?} not found"))?;
        a.read(e)?
    };
    let n = names(install, &vfs);
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".scene") {
        let scene = ttmeta::Scene::parse(&bytes)?;
        println!("scene {} ({} agents)", scene.name, scene.agents.len());
        for ag in &scene.agents {
            println!("  agent {}", ag.name);
            show_props(&n, &ag.props, "    ");
        }
    } else if lower.ends_with(".prop") {
        show_props(&n, &ttmeta::PropertySet::from_file(&bytes)?, "  ");
    } else if lower.ends_with(".d3dmesh") {
        let m = ttmeta::Mesh::parse(&bytes)?;
        println!("{} bounds {:?} verts {} tris {}", m.name, m.bounds, m.positions.len(), m.indices.len() / 3);
        for s in &m.submeshes {
            println!("  submesh first {} count {} textures {:?}", s.first_index, s.index_count, s.textures);
        }
        println!("  texture groups {:?}", m.textures);
    } else if lower.ends_with(".d3dtx") {
        let t = ttmeta::Texture::parse(&bytes)?;
        println!("{} {}x{} {:?} mips {}", t.name, t.width, t.height, t.format, t.mips.len());
    } else if lower.ends_with(".font") {
        let f = ttmeta::Font::parse(&bytes)?;
        println!("{} size {} base {} glyphs {} pages {}", f.name, f.size, f.base, f.glyphs.len(), f.pages.len());
    } else if lower.ends_with(".anm") {
        let a = ttmeta::Animation::parse(&bytes)?;
        println!("length {} values {} complete {}", a.length, a.values.len(), a.complete);
        for v in &a.values {
            let keys = match &v.track {
                ttmeta::anim::Track::Transform(k) => format!("transform x{}", k.len()),
                ttmeta::anim::Track::Float(k) => format!("float x{}", k.len()),
                ttmeta::anim::Track::Bool(k) => format!("bool x{}", k.len()),
                other => format!("{other:?}").chars().take(30).collect(),
            };
            println!("  {} flags {:#x} {keys}", n.show(v.name), v.flags);
        }
    } else {
        return Err("meta supports .scene .prop .d3dmesh .d3dtx .font .anm".into());
    }
    Ok(())
}

/// Write a decrypted script (Lua 5.2 bytecode) for study with an external decompiler.
fn script(install: &Install, name: &str, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let out = output_dir(out)?;
    let loose = Path::new(name);
    let (file_name, bytes) = if loose.is_file() {
        (loose.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(loose)?)
    } else {
        let vfs = install.mount()?;
        let (a, e) = vfs.find(name).ok_or_else(|| format!("{name:?} not found"))?;
        (e.name.clone(), a.read(e)?)
    };
    let plain = ttarch::script::decrypt(&install.cipher, &bytes);
    let dest = out.join(format!("{file_name}c"));
    std::fs::write(&dest, &plain)?;
    println!("{} bytes -> {}", plain.len(), dest.display());
    Ok(())
}

fn extract(install: &Install, name: &str, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let out = output_dir(out)?;
    let vfs = install.mount()?;
    let (a, e) = vfs.find(name).ok_or_else(|| format!("{name:?} not found"))?;
    let bytes = a.read(e)?;
    let dest = out.join(Path::new(&e.name).file_name().unwrap());
    std::fs::write(&dest, &bytes)?;
    println!("{} bytes -> {}", bytes.len(), dest.display());
    Ok(())
}
