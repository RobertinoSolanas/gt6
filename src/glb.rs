//! GLB (binary glTF) model loading.
//!
//! Uses the `gltf` crate (Khronos Group's reference Rust implementation) to
//! traverse the node hierarchy of a GLB file and extract vertex buffers,
//! normals, UVs, indices, material base colors and embedded textures
//! directly into plain Rust structs. GLB is the binary single-file version
//! of glTF: mesh data + textures packed into one file, so a model is just
//! one asset on disk (export as GLB from Blender to get this for free).
//!
//! Pure Rust (no DOM), unit-testable on the host.

/// A decoded RGB texture, row-major, 3 bytes per pixel.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbTex {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl RgbTex {
    /// Bilinear sample. `u` wraps (textures tile), `v` clamps.
    /// glTF UVs have `v = 0` at the top of the image.
    pub fn sample(&self, u: f64, v: f64) -> [u8; 3] {
        if self.px.is_empty() {
            return [255, 255, 255];
        }
        // u wraps (texture tiling); `u >= 1.0` clamps to the right edge
        // (rem_euclid would send it back to the left edge).
        let u = if u >= 1.0 { 1.0 } else { u.rem_euclid(1.0) };
        let v = v.clamp(0.0, 1.0 - 1e-6);
        let fx = u * (self.w as f64 - 1.0);
        let fy = v * (self.h as f64 - 1.0);
        let x0 = fx.floor() as i64;
        let y0 = fy.floor() as i64;
        let tx = fx - x0 as f64;
        let ty = fy - y0 as f64;
        let px = |x: i64, y: i64| -> [u8; 3] {
            let x = ((x % self.w as i64) + self.w as i64) % self.w as i64;
            let y = y.clamp(0, self.h as i64 - 1);
            let i = ((y as u32) * self.w + x as u32) as usize * 3;
            [self.px[i], self.px[i + 1], self.px[i + 2]]
        };
        let c00 = px(x0, y0);
        let c10 = px(x0 + 1, y0);
        let c01 = px(x0, y0 + 1);
        let c11 = px(x0 + 1, y0 + 1);
        [0, 1, 2].map(|i| {
            let a = (c00[i] as f64 * (1.0 - tx) + c10[i] as f64 * tx) * (1.0 - ty)
                + (c01[i] as f64 * (1.0 - tx) + c11[i] as f64 * tx) * ty;
            a.round().clamp(0.0, 255.0) as u8
        })
    }

    /// The average color of the texture (fallback when UVs are missing).
    pub fn average(&self) -> [u8; 3] {
        if self.px.is_empty() {
            return [255, 255, 255];
        }
        let mut acc = [0.0f64; 3];
        for i in 0..self.px.len() {
            acc[i % 3] += self.px[i] as f64;
        }
        let n = (self.px.len() / 3).max(1) as f64;
        [0, 1, 2].map(|i| (acc[i] / n).round() as u8)
    }
}

/// One mesh (all primitives merged) with its material color / texture.
#[derive(Clone, Debug)]
pub struct GltfMesh {
    pub name: String,
    /// Vertex positions (model units, y-up per the glTF spec).
    pub positions: Vec<[f64; 3]>,
    /// Per-vertex normals (normalized), or (0,0,1) if the model has none.
    pub normals: Vec<[f64; 3]>,
    /// Per-vertex UVs (0,0) if the mesh has no TEXCOORD_0.
    pub uvs: Vec<[f64; 2]>,
    /// Triangle indices into `positions`.
    pub indices: Vec<u32>,
    /// Material base color factor (linear RGB, 0..1).
    pub base_color: [f64; 3],
    /// The base color texture, if the material has one.
    pub texture: Option<RgbTex>,
}

impl GltfMesh {
    pub fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The base RGB color (0xRRGGBB) of a vertex: texture sampled at its UV,
    /// multiplied by the material base color factor.
    pub fn vertex_color(&self, i: usize) -> [u8; 3] {
        let base = [
            (self.base_color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.base_color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.base_color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        ];
        let t = match &self.texture {
            Some(tex) => tex.sample(self.uvs[i][0], self.uvs[i][1]),
            None => base,
        };
        [
            (t[0] as u32 * base[0] as u32 / 255) as u8,
            (t[1] as u32 * base[1] as u32 / 255) as u8,
            (t[2] as u32 * base[2] as u32 / 255) as u8,
        ]
    }
}

/// A parsed GLB: every mesh in the default scene, node transforms applied.
#[derive(Clone, Debug)]
pub struct GltfModel {
    pub meshes: Vec<GltfMesh>,
}

impl GltfModel {
    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    pub fn tri_count(&self) -> usize {
        self.meshes.iter().map(|m| m.tri_count()).sum()
    }
}

/// Turn gltf-decoded raw pixels into an RGB texture, downscaled so its
/// largest side is at most `max_side` px (keeps the wasm memory small; the
/// software rasterizer only needs per-triangle color samples).
fn decode_pixels(px: &[u8], format: gltf::image::Format, w: u32, h: u32, max_side: u32) -> Option<RgbTex> {
    let img: image::DynamicImage = match format {
        gltf::image::Format::R8G8B8 => image::RgbImage::from_raw(w, h, px.to_vec())?.into(),
        gltf::image::Format::R8G8B8A8 => image::RgbaImage::from_raw(w, h, px.to_vec())?.into(),
        _ => return None,
    };
    let img = img.resize_to_fill(max_side, max_side, image::imageops::FilterType::Lanczos3).to_rgb8();
    Some(RgbTex { w: img.width(), h: img.height(), px: img.into_raw() })
}

/// Parse a GLB (binary glTF) file. Returns the meshes of the default scene
/// with node transforms applied and textures decoded.
pub fn load_glb(bytes: &[u8]) -> Result<GltfModel, String> {
    let (doc, buffers, images) =
        gltf::import_slice(bytes).map_err(|e| format!("glTF parse failed: {e}"))?;
    let buffers: Vec<Vec<u8>> = buffers.into_iter().map(|d| d.0).collect();

    let Some(scene) = doc.default_scene() else {
        return Err("glTF has no default scene".into());
    };
    let mut model = GltfModel { meshes: Vec::new() };
    for node in scene.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        // Node transform (applied to positions and normals).
        let t = node.transform();
        let xform_pos = |v: [f64; 3]| -> [f64; 3] {
            match &t {
                gltf::scene::Transform::Decomposed { translation, rotation, scale } => {
                    let r = rotate_quat(v, rotation);
                    [
                        r[0] * scale[0] as f64 + translation[0] as f64,
                        r[1] * scale[1] as f64 + translation[1] as f64,
                        r[2] * scale[2] as f64 + translation[2] as f64,
                    ]
                }
                gltf::scene::Transform::Matrix { matrix } => {
                    let m = matrix;
                    [
                        m[0][0] as f64 * v[0] + m[1][0] as f64 * v[1] + m[2][0] as f64 * v[2] + m[3][0] as f64,
                        m[0][1] as f64 * v[0] + m[1][1] as f64 * v[1] + m[2][1] as f64 * v[2] + m[3][1] as f64,
                        m[0][2] as f64 * v[0] + m[1][2] as f64 * v[1] + m[2][2] as f64 * v[2] + m[3][2] as f64,
                    ]
                }
            }
        };
        let xform_nrm = |v: [f64; 3]| -> [f64; 3] {
            let r = match &t {
                gltf::scene::Transform::Decomposed { rotation, .. } => rotate_quat(v, rotation),
                gltf::scene::Transform::Matrix { matrix } => {
                    let m = matrix;
                    [
                        m[0][0] as f64 * v[0] + m[1][0] as f64 * v[1] + m[2][0] as f64 * v[2],
                        m[0][1] as f64 * v[0] + m[1][1] as f64 * v[1] + m[2][1] as f64 * v[2],
                        m[0][2] as f64 * v[0] + m[1][2] as f64 * v[1] + m[2][2] as f64 * v[2],
                    ]
                }
            };
            let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
            if l > 1e-12 {
                [r[0] / l, r[1] / l, r[2] / l]
            } else {
                v
            }
        };

        let mut positions: Vec<[f64; 3]> = Vec::new();
        let mut normals: Vec<[f64; 3]> = Vec::new();
        let mut uvs: Vec<[f64; 2]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut base_color: [f64; 3] = [1.0, 1.0, 1.0];
        let mut texture: Option<RgbTex> = None;

        for prim in mesh.primitives() {
            let reader = prim.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
            let base = positions.len() as u32;
            let Some(p) = reader.read_positions() else { continue };
            let n_pos = p.count();
            for v in reader.read_positions().unwrap() {
                positions.push(xform_pos([v[0] as f64, v[1] as f64, v[2] as f64]));
            }
            if let Some(n) = reader.read_normals() {
                for v in n {
                    normals.push(xform_nrm([v[0] as f64, v[1] as f64, v[2] as f64]));
                }
            }
            if let Some(uv) = reader.read_tex_coords(0) {
                for v in uv.into_f32() {
                    uvs.push([v[0] as f64, v[1] as f64]);
                }
            }
            let _ = n_pos; // (kept for clarity; gaps are fixed below)
            let Some(ri) = reader.read_indices() else {
                continue;
            };
            for i in ri.into_u32() {
                indices.push(base + i);
            }
            let pbr = prim.material().pbr_metallic_roughness();
            let fc = pbr.base_color_factor();
            base_color = [fc[0] as f64, fc[1] as f64, fc[2] as f64];
            if let Some(tex) = pbr.base_color_texture() {
                let img_idx = tex.texture().source().index();
                if let Some(d) = images.get(img_idx) {
                    if let Some(t) = decode_pixels(&d.pixels, d.format, d.width, d.height, 256) {
                        texture = Some(t);
                    }
                }
            }
        }

        if positions.is_empty() {
            continue;
        }
        // Fill gaps when an accessor was missing (keep index alignment).
        if normals.len() != positions.len() {
            normals = vec![[0.0, 0.0, 1.0]; positions.len()];
        }
        if uvs.len() != positions.len() {
            uvs = vec![[0.0, 0.0]; positions.len()];
        }
        model.meshes.push(GltfMesh {
            name: mesh.name().unwrap_or("mesh").to_string(),
            positions,
            normals,
            uvs,
            indices,
            base_color,
            texture,
        });
    }
    Ok(model)
}

/// Rotate a vector by a glTF quaternion (xyzw, w may be 0).
/// Uses v' = v + 2w(q × v) + 2(q × (q × v)) for a unit quaternion.
fn rotate_quat(v: [f64; 3], q: &[f32; 4]) -> [f64; 3] {
    let (qx, qy, qz, w) = (q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64);
    let (vx, vy, vz) = (v[0], v[1], v[2]);
    // q × v
    let cx = qy * vz - qz * vy;
    let cy = qz * vx - qx * vz;
    let cz = qx * vy - qy * vx;
    // q × (q × v)
    let dx = qy * cz - qz * cy;
    let dy = qz * cx - qx * cz;
    let dz = qx * cy - qy * cx;
    [
        vx + 2.0 * (w * cx + dx),
        vy + 2.0 * (w * cy + dy),
        vz + 2.0 * (w * cz + dz),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny GLB in memory: one triangle, one 2x2 JPEG texture.
    fn tiny_glb() -> Vec<u8> {
        // 2x2 JPEG (red / green / blue / white).
        let mut jpeg = Vec::new();
        {
            let px = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
            let img = image::RgbImage::from_raw(2, 2, px.to_vec()).unwrap();
            image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
                .encode(img.as_raw(), 2, 2, image::ExtendedColorType::Rgb8)
                .unwrap();
        }

        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0}}],
  "meshes": [{{
    "name": "Tri",
    "primitives": [{{
      "attributes": {{"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}},
      "indices": 3,
      "material": 0
    }}]
  }}],
  "materials": [{{
    "name": "Mat",
    "pbrMetallicRoughness": {{"baseColorFactor": [1.0, 1.0, 1.0, 1.0], "baseColorTexture": {{"index": 0}}}}
  }}],
  "textures": [{{"source": 0}}],
  "images": [{{"bufferView": 4, "mimeType": "image/jpeg"}}],
  "buffers": [{{"byteLength": {}}}],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 36, "target": 34962}},
    {{"buffer": 0, "byteOffset": 72, "byteLength": 24, "target": 34962}},
    {{"buffer": 0, "byteOffset": 96, "byteLength": 12, "target": 34963}},
    {{"buffer": 0, "byteOffset": 112, "byteLength": {}}}
  ],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]}},
    {{"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2"}},
    {{"bufferView": 3, "componentType": 5125, "count": 3, "type": "SCALAR", "min": [0], "max": [2]}}
  ]
}}"#,
            112 + jpeg.len(),
            jpeg.len()
        );
        let mut json = json.into_bytes();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }

        let mut bin = Vec::new();
        // Positions: three triangle verts.
        for &v in &[0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        // Normals: all up (0, 0, 1).
        for _ in 0..3 {
            for v in [0.0f32, 0.0, 1.0] {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        // UVs.
        for &v in &[0.0f32, 0.0, 1.0, 0.0, 0.5, 1.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        // Indices (u32).
        for v in [0u32, 1, 2] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        // Pad to the JPEG's 4-aligned offset (112).
        while bin.len() < 112 {
            bin.push(0);
        }
        bin.extend_from_slice(&jpeg);
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F534A_u32.to_le_bytes()); // "JSON"
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942_u32.to_le_bytes()); // "BIN\0"
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn parses_a_minimal_glb() {
        let bytes = tiny_glb();
        let model = load_glb(&bytes).expect("should parse");
        assert_eq!(model.meshes.len(), 1);
        let m = &model.meshes[0];
        assert_eq!(m.name, "Tri");
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.indices, vec![0, 1, 2]);
        assert_eq!(m.positions[1], [1.0, 0.0, 0.0]);
        assert!(m.texture.is_some(), "embedded JPEG texture should decode");
        let tex = m.texture.as_ref().unwrap();
        assert!(tex.w >= 1 && tex.h >= 1);
        // Vertex 0 is at uv (0,0): the red corner.
        let c = m.vertex_color(0);
        assert!(c[0] > 128 && c[1] < 128 && c[2] < 128, "expected red, got {c:?}");
    }

    #[test]
    fn rejects_garbage() {
        assert!(load_glb(b"not a glb at all").is_err());
        assert!(load_glb(b"").is_err());
    }

    #[test]
    fn tex_sample_and_average() {
        // 2x2: left column red, right column blue.
        let tex = RgbTex {
            w: 2,
            h: 2,
            px: vec![255, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 255],
        };
        let c = tex.sample(0.0, 0.5);
        assert!(c[0] > 200 && c[2] < 55, "left edge is red: {c:?}");
        let c = tex.sample(1.0, 0.5);
        assert!(c[2] > 200 && c[0] < 55, "right edge is blue: {c:?}");
        // u wraps.
        let c = tex.sample(1.5, 0.5);
        assert!(c[2] > 200, "wraps: {c:?}");
        let avg = tex.average();
        assert!(avg[0] > 60 && avg[2] > 60);
    }
}
