//! Print each chore resource's control tracks: `cargo run -p ttmeta --example chore_tracks -- <file.chore> [agent-substring]`.
use ttmeta::anim::Track;
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("chore path");
    let filter = args.next().unwrap_or_default();
    let c = ttmeta::Chore::parse(&std::fs::read(path).unwrap()).unwrap();
    for a in c.agents.iter().filter(|a| a.name.contains(&filter)) {
        for &i in &a.resources {
            let r = &c.resources[i];
            for v in &r.control.values {
                let desc = match &v.track {
                    Track::Transform(k) => format!(
                        "transform x{} first {:?} last {:?} times {:?}",
                        k.len(),
                        k.first().map(|k| k.value.pos),
                        k.last().map(|k| k.value.pos),
                        k.iter().map(|k| (k.time, k.interpolate, k.tangent)).collect::<Vec<_>>()
                    ),
                    Track::Float(k) => format!("float x{} {:?}", k.len(), k.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>()),
                    Track::Bool(k) => format!("bool x{}", k.len()),
                    Track::Color(k) => format!("color x{} {:?}", k.len(), k.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>()),
                    _ => "other".into(),
                };
                println!("{}\t#{:016x}\t#{:016x}\t{desc}", a.name, r.name.0, v.name.0);
            }
        }
    }
}
