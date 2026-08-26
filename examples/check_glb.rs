//! Dev tool: load a GLB file and print its structure (meshes, triangle
//! counts, textures) and, for the dragon asset, the baked mesh stats.
//!
//! ```sh
//! cargo run --example check_glb web/assets/dragon.glb
//! ```

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: check_glb <file.glb>");
        std::process::exit(2);
    });
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {path}: {e}");
        std::process::exit(1);
    });
    let model = gt6::glb::load_glb(&bytes).unwrap_or_else(|e| {
        eprintln!("parse failed: {e}");
        std::process::exit(1);
    });
    println!("{}: {} mesh(es), {} tris total", path, model.meshes.len(), model.tri_count());
    for m in &model.meshes {
        let tex = match &m.texture {
            Some(t) => format!("texture {}x{}", t.w, t.h),
            None => "no texture".to_string(),
        };
        println!(
            "  - {:<20} {:>7} tris  {:>6} verts  base {:#08x}  {}",
            m.name,
            m.tri_count(),
            m.positions.len(),
            (((m.base_color[0].clamp(0.0, 1.0) * 255.0) as u32) << 16)
                | (((m.base_color[1].clamp(0.0, 1.0) * 255.0) as u32) << 8)
                | (m.base_color[2].clamp(0.0, 1.0) * 255.0) as u32,
            tex,
        );
    }
    if let Some(dm) = gt6::wildlife::DragonMesh::from_gltf(&model) {
        println!(
            "baked dragon: {} tris, half_span {:.1}, z {}..{}",
            dm.tri_count(),
            dm.half_span,
            dm.z_min,
            dm.z_max
        );
        // Average baked color, for a sanity check on the texture path.
        let acc: Vec<u32> = dm.vcol.iter().copied().collect();
        let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
        for c in &acc {
            r += (c >> 16) as u64;
            g += (c >> 8) as u64;
            b += (c & 0xff) as u64;
        }
        let n = acc.len().max(1) as u64;
        println!("avg vertex color: {:#08x}", (r / n) << 16 | (g / n) << 8 | b / n);
    } else {
        println!("baked dragon: none (no mesh named *dragon*)");
    }
}
