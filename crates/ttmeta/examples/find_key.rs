//! List props (archived, or a loose file) containing a key: `cargo run -p ttmeta --example find_key -- <game-dir> <key> [file]`.
fn main() {
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game dir");
    let key = ttmeta::crc64(&args.next().expect("key")).to_le_bytes();
    let vfs = ttarch::Install::open(game.as_ref()).unwrap().mount().unwrap();
    for a in &vfs.archives {
        for e in a.entries().iter().filter(|e| e.name.to_ascii_lowercase().ends_with(".prop")) {
            let b = a.read(e).unwrap();
            if b.windows(8).any(|w| w == key) {
                println!("{}", e.name);
            }
        }
    }
}
