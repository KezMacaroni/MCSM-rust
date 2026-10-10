//! List archived file names containing a substring: `cargo run -p ttmeta --example names -- <game-dir> <substring>`.
fn main() {
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game dir");
    let pat = args.next().expect("substring").to_ascii_lowercase();
    let vfs = ttarch::Install::open(game.as_ref()).unwrap().mount().unwrap();
    let mut names: Vec<_> = vfs.archives.iter().flat_map(|a| a.entries().iter().map(|e| e.name.clone())).filter(|n| n.to_ascii_lowercase().contains(&pat)).collect();
    names.sort();
    names.dedup();
    for n in names {
        println!("{n}");
    }
}
