//! Resolve menu labels: `cargo run -p ttmeta --example labels -- <game-dir> <label>...`.
fn main() {
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game dir");
    let vfs = ttarch::Install::open(game.as_ref()).unwrap().mount().unwrap();
    let read = |n: &str| vfs.find(n).map(|(a, e)| a.read(e).unwrap()).unwrap();
    let dlog = read("ui_menu.dlog");
    let db = ttmeta::text::LanguageDb::parse(&read("ui_menu_english.landb"));
    for label in args {
        let text = ttmeta::text::dialog_label(&dlog, &label, &db).and_then(|id| db.get(id));
        println!("{label:32} {text:?}");
    }
}
