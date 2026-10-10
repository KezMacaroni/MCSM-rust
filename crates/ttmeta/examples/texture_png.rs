//! Decode a texture's top mip to a PPM over grey (debugging aid; outside the repo):
//! `cargo run -p ttmeta --example texture_png -- <game-dir> <name.d3dtx> <out.ppm>`.
fn main() {
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game dir");
    let name = args.next().expect("texture");
    let out = args.next().expect("output");
    let vfs = ttarch::Install::open(game.as_ref()).unwrap().mount().unwrap();
    let (a, e) = vfs.find(&name).expect("texture not found");
    let t = ttmeta::Texture::parse(&a.read(e).unwrap()).unwrap();
    let rgba = t.level0_rgba().expect("format not decodable");
    let mut ppm = format!("P6 {} {} 255\n", t.width, t.height).into_bytes();
    for p in rgba.chunks_exact(4) {
        let a = p[3] as u32;
        for c in 0..3 {
            ppm.push(((p[c] as u32 * a + 128 * (255 - a)) / 255) as u8);
        }
    }
    std::fs::write(out, ppm).unwrap();
    // ALPHA_REGION=u0,v0,u1,v1: print the average alpha there.
    if let Ok(r) = std::env::var("ALPHA_REGION") {
        let v: Vec<f32> = r.split(',').map(|x| x.parse().unwrap()).collect();
        let (w, h) = (t.width as f32, t.height as f32);
        let (mut sum, mut n, mut min) = (0u64, 0u64, 255u8);
        for y in (v[1] * h) as u32..(v[3] * h) as u32 {
            for x in (v[0] * w) as u32..(v[2] * w) as u32 {
                let a = rgba[((y * t.width + x) * 4 + 3) as usize];
                sum += a as u64;
                n += 1;
                min = min.min(a);
            }
        }
        println!("alpha avg {} min {}", sum / n.max(1), min);
    }
}
