//! Print a chore's agents and resources: `cargo run -p ttmeta --example chore_info -- <file.chore>`,
//! including whether each embedded animation decoded.
fn main() {
    let path = std::env::args().nth(1).expect("chore path");
    let c = ttmeta::Chore::parse(&std::fs::read(path).unwrap()).unwrap();
    println!("{} length {} scene {}", c.name, c.length, c.scene);
    for a in &c.agents {
        for &i in &a.resources {
            let r = &c.resources[i];
            let blocks: Vec<_> = r.blocks.iter().map(|b| (b.start, b.end, b.looping, b.scale)).collect();
            println!("{}\t#{:016x}\tlen {}\ton {}\t{:?}\tprio {}\tflags {:#x}\tblocks {:?}", a.name, r.name.0, r.length, r.enabled, r.bools.map(|b| b as u8), r.priority, r.flags, blocks);
        }
    }
}
