//! List every language string near each reference to a dialog label's node (debugging aid):
//! `cargo run -p ttmeta --example label_probe -- <game-dir> <label>`.
fn main() {
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game dir");
    let label = args.next().expect("label");
    let vfs = ttarch::Install::open(game.as_ref()).unwrap().mount().unwrap();
    let read = |n: &str| vfs.find(n).map(|(a, e)| a.read(e).unwrap()).unwrap();
    let dlog = read("ui_menu.dlog");
    let db = ttmeta::text::LanguageDb::parse(&read("ui_menu_english.landb"));
    let name = ttmeta::crc64(&label).to_le_bytes();
    let at = dlog.windows(8).position(|w| w == name).unwrap();
    let obj = &dlog[at - 12..at - 4];
    for (hit, _) in dlog.windows(8).enumerate().filter(|(_, w)| *w == obj) {
        let window = &dlog[hit + 8..(hit + 0x400).min(dlog.len())];
        let found: Vec<_> = (0..window.len() - 4)
            .filter_map(|k| {
                let id = u32::from_le_bytes(window[k..k + 4].try_into().unwrap());
                db.get(id).map(|t| (k, id, t.to_owned()))
            })
            .collect();
        println!("ref {hit:#x}: {found:?}");
    }
}
