//! Inspect supported dialogue nodes without extracting assets into the repository.
fn main() {
    let path = std::env::args().nth(1).expect("dialog path");
    let bytes = std::fs::read(path).unwrap();
    let dialog = ttmeta::dialog::Dialog::parse(&bytes).unwrap();
    println!("{} supported nodes", dialog.nodes.len());
    let mut nodes: Vec<_> = dialog.nodes.values().collect();
    nodes.sort_by_key(|n| n.id.0);
    for node in nodes {
        println!("{:?} name {:?} next {:?} {:?}", node.id, node.name, node.next, node.kind);
        for (key, _, value) in &node.props.values { println!("  {key:?}: {value:?}"); }
    }
}
