//! List the install's resource sets: `cargo run -p ttarch --example sets -- <game-dir>`.
//! Name, priority and enable mode, highest priority first.
fn main() {
    let game = std::env::args().nth(1).expect("game dir");
    let install = ttarch::Install::open(game.as_ref()).unwrap();
    for s in ttarch::resdesc::load(&install.archives_dir(), &install.cipher).unwrap() {
        println!("{:>6}  {:<12} {}", s.priority, s.enable_mode, s.name);
    }
}
