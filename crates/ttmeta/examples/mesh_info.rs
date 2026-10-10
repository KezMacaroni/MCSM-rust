//! Print a mesh's submeshes and skinning summary: `cargo run -p ttmeta --example mesh_info -- <file.d3dmesh>`.
fn main() {
    let path = std::env::args().nth(1).expect("mesh path");
    let m = ttmeta::Mesh::parse(&std::fs::read(path).unwrap()).unwrap();
    println!("{} vertices {} joints {} weights {} palettes {:?}", m.name, m.positions.len(), m.joints.len(), m.weights.len(), m.palettes.iter().map(Vec::len).collect::<Vec<_>>());
    for s in &m.submeshes {
        println!("  submesh first {} count {} palette {:?} blend {:?} tint {:?}", s.first_index, s.index_count, s.palette, s.blend, s.tint);
    }
    println!("  first joints {:?} weights {:?}", m.joints.first(), m.weights.first());
}
