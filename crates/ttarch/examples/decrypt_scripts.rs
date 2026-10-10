//! Decrypt every `.lua` in the install into a directory outside the repo:
//! `cargo run -p ttarch --example decrypt_scripts -- <game-dir> <out-dir>`.
fn main() {
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game dir");
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    let install = ttarch::Install::open(game.as_ref()).unwrap();
    let vfs = install.mount().unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut n = 0;
    for a in &vfs.archives {
        for e in a.entries().iter().filter(|e| e.name.to_ascii_lowercase().ends_with(".lua")) {
            let plain = ttarch::script::decrypt(&install.cipher, &a.read(e).unwrap());
            std::fs::write(out.join(format!("{}c", e.name)), plain).unwrap();
            n += 1;
        }
    }
    println!("{n} scripts");
}
