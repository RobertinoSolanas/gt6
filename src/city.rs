//! Procedural city: a grid of blocks separated by roads.
//!
//! Layout (world units = pixels):
//!   [road][block][road][block] ... [road]
//! Roads run along both axes at multiples of `CELL`, each `ROAD` wide.
//! Blocks contain a 2x2 grid of building lots (with a walkable sidewalk
//! band around them) or, occasionally, a park.

use crate::Rng;

pub const CELL: f64 = 280.0; // block + road
pub const BLOCK: f64 = 200.0;
pub const ROAD: f64 = 80.0;
pub const N: usize = 9; // blocks per axis
pub const SIDEWALK: f64 = 22.0;

/// Total city size.
pub const SIZE: f64 = ROAD + N as f64 * CELL;
/// Road centers (intersection coordinates) along one axis: 0..=N.
pub const LANES: usize = N + 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockKind {
    Buildings,
    Park,
}

/// A building rectangle (collision box).
#[derive(Clone, Copy, Debug)]
pub struct Building {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub color: u32, // 0xRRGGBB
}

impl Building {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// A park: green block with non-colliding trees.
#[derive(Clone, Copy, Debug)]
pub struct Park {
    pub x: f64,
    pub y: f64,
    pub trees: [(f64, f64, f64); 5], // cx, cy, radius
}

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub kind: BlockKind,
    pub buildings: [Option<Building>; 4],
    pub park: Option<Park>,
}

pub struct City {
    pub blocks: Vec<Block>, // index j * N + i
}

const BUILDING_PALETTE: [u32; 8] = [
    0x8d99ae, 0xb5838d, 0x6d6875, 0x9a8c98,
    0x84a98c, 0xc9ada7, 0x5f7470, 0xa68a64,
];

impl City {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut blocks = Vec::with_capacity(N * N);
        for j in 0..N {
            for i in 0..N {
                let bx = i as f64 * CELL + ROAD;
                let by = j as f64 * CELL + ROAD;
                let is_park = rng.f() < 0.12;
                let block = if is_park {
                    let mut trees = [(0.0, 0.0, 0.0); 5];
                    for t in trees.iter_mut() {
                        t.0 = bx + rng.range(30.0, BLOCK - 30.0);
                        t.1 = by + rng.range(30.0, BLOCK - 30.0);
                        t.2 = rng.range(10.0, 18.0);
                    }
                    Block {
                        kind: BlockKind::Park,
                        buildings: [None; 4],
                        park: Some(Park { x: bx, y: by, trees }),
                    }
                } else {
                    let mut buildings = [None; 4];
                    for lot in 0..4 {
                        if rng.f() < 0.15 {
                            continue; // empty lot
                        }
                        let li = lot % 2;
                        let lj = lot / 2;
                        let lot_w = (BLOCK - 2.0 * SIDEWALK) / 2.0;
                        let x = bx + SIDEWALK + li as f64 * lot_w;
                        let y = by + SIDEWALK + lj as f64 * lot_w;
                        let inset = rng.range(4.0, 12.0);
                        let w = lot_w - 4.0 - inset;
                        let h = lot_w - 4.0 - inset;
                        let color = BUILDING_PALETTE[rng.below(BUILDING_PALETTE.len())];
                        buildings[lot] = Some(Building { x, y, w, h, color });
                    }
                    Block {
                        kind: BlockKind::Buildings,
                        buildings,
                        park: None,
                    }
                };
                blocks.push(block);
            }
        }
        City { blocks }
    }

    pub fn block(&self, i: usize, j: usize) -> &Block {
        &self.blocks[j * N + i]
    }

    /// Block index for a point, or None if the point is on a road.
    pub fn block_at_point(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        for j in 0..N {
            let by = j as f64 * CELL + ROAD;
            if y < by || y > by + BLOCK {
                continue;
            }
            for i in 0..N {
                let bx = i as f64 * CELL + ROAD;
                if x >= bx && x <= bx + BLOCK {
                    return Some((i, j));
                }
            }
        }
        None
    }

    /// Is (x, y) on any road?
    pub fn is_road(&self, x: f64, y: f64) -> bool {
        self.block_at_point(x, y).is_none()
    }

    /// Intersection centers are at (i * CELL + ROAD/2, j * CELL + ROAD/2).
    pub fn intersection_pos(i: usize, j: usize) -> (f64, f64) {
        (i as f64 * CELL + ROAD / 2.0, j as f64 * CELL + ROAD / 2.0)
    }

    /// All buildings in the city.
    pub fn buildings(&self) -> impl Iterator<Item = &Building> {
        self.blocks.iter().flat_map(|b| b.buildings.iter()).filter_map(Option::as_ref)
    }

    /// Circle-vs-buildings (+ city bounds) collision.
    /// Returns `(x, y, nx, ny)` = corrected position + collision normal if the
    /// circle at `(x, y)` with radius `r` penetrates anything.
    pub fn collide_circle(&self, x: f64, y: f64, r: f64) -> Option<(f64, f64, f64, f64)> {
        // City bounds (outer road is drivable/walkable, but not beyond it).
        let m = r;
        let (mut x, mut y) = (x, y);
        let mut resolved = false;
        if x < m {
            x = m;
            resolved = true;
        }
        if x > SIZE - m {
            x = SIZE - m;
            resolved = true;
        }
        if y < m {
            y = m;
            resolved = true;
        }
        if y > SIZE - m {
            y = SIZE - m;
            resolved = true;
        }
        if !resolved {
            return None;
        }
        let nx = if x != 0.0 && x != SIZE { 0.0 } else { -1.0 }; // approx, unused for bounds
        let ny = if y != 0.0 && y != SIZE { 0.0 } else { -1.0 };
        // Re-check buildings after bounds fix.
        if let Some(hit) = self.collide_buildings(x, y, r) {
            return Some(hit);
        }
        Some((x, y, nx, ny))
    }

    /// Circle vs building rectangles only. Returns (x, y, nx, ny).
    pub fn collide_buildings(&self, x: f64, y: f64, r: f64) -> Option<(f64, f64, f64, f64)> {
        let mut x = x;
        let mut y = y;
        let mut hit = None;
        for b in self.buildings() {
            // Broad phase: skip rects far away.
            if x + r < b.x || x - r > b.x + b.w || y + r < b.y || y - r > b.y + b.h {
                continue;
            }
            let cx = x.min(b.x + b.w).max(b.x);
            let cy = y.min(b.y + b.h).max(b.y);
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 >= r * r {
                continue;
            }
            if d2 > 1e-9 {
                let d = d2.sqrt();
                let nx = dx / d;
                let ny = dy / d;
                let push = r - d;
                x += nx * push;
                y += ny * push;
                hit = Some((x, y, nx, ny));
            } else {
                // Center inside the rect: push out along smallest axis.
                let left = x - b.x;
                let right = b.x + b.w - x;
                let top = y - b.y;
                let bottom = b.y + b.h - y;
                let min = left.min(right).min(top).min(bottom);
                let (nx, ny) = if min == left {
                    (-1.0, 0.0)
                } else if min == right {
                    (1.0, 0.0)
                } else if min == top {
                    (0.0, -1.0)
                } else {
                    (0.0, 1.0)
                };
                x += nx * (min + r);
                y += ny * (min + r);
                hit = Some((x, y, nx, ny));
            }
        }
        hit
    }

    /// Random point on a road, at least `min_dist` away from (fx, fy).
    pub fn random_road_point(&self, rng: &mut Rng, fx: f64, fy: f64, min_dist: f64) -> (f64, f64) {
        for _ in 0..64 {
            let i = rng.below(LANES);
            let j = rng.below(LANES);
            let (mut x, mut y) = Self::intersection_pos(i, j);
            // Jitter along one of the roads through the intersection, staying
            // within the road width (road center ± ROAD/2 - margin).
            let along = rng.range(-ROAD / 2.0 + 10.0, ROAD / 2.0 - 10.0);
            if rng.below(2) == 0 {
                x += along;
            } else {
                y += along;
            }
            x = x.max(10.0).min(SIZE - 10.0);
            y = y.max(10.0).min(SIZE - 10.0);
            let dx = x - fx;
            let dy = y - fy;
            if dx * dx + dy * dy >= min_dist * min_dist {
                return (x, y);
            }
        }
        // Fallback: the farthest city corner road point (always far enough
        // in practice and always on a road).
        let corners = [(10.0, 10.0), (SIZE - 10.0, 10.0), (10.0, SIZE - 10.0), (SIZE - 10.0, SIZE - 10.0)];
        corners.iter().copied().max_by(|a, b| {
            let da = (a.0 - fx).powi(2) + (a.1 - fy).powi(2);
            let db = (b.0 - fx).powi(2) + (b.1 - fy).powi(2);
            da.partial_cmp(&db).unwrap()
        }).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn city_size_is_consistent() {
        assert_eq!(SIZE, ROAD + N as f64 * CELL);
        assert!(SIZE > 0.0);
    }

    #[test]
    fn roads_and_blocks_partition_the_grid() {
        let city = City::new(42);
        // Intersection centers are on roads.
        for i in 0..=N {
            for j in 0..=N {
                let (x, y) = City::intersection_pos(i, j);
                assert!(city.is_road(x, y), "({x},{y}) should be a road");
            }
        }
        // Block centers (with buildings) are not roads.
        let mut building_blocks = 0;
        for j in 0..N {
            for i in 0..N {
                let b = city.block(i, j);
                match b.kind {
                    BlockKind::Buildings => building_blocks += 1,
                    BlockKind::Park => {}
                }
                let (x, y) = (i as f64 * CELL + ROAD + BLOCK / 2.0, j as f64 * CELL + ROAD + BLOCK / 2.0);
                assert!(!city.is_road(x, y), "block center should be a block");
            }
        }
        // With a fixed seed we expect a healthy mix, and enough buildings.
        assert!(building_blocks >= N * N / 2);
        assert!(city.buildings().count() > 50);
    }

    #[test]
    fn buildings_do_not_overlap_roads() {
        let city = City::new(7);
        for b in city.buildings() {
            // A building must fit fully inside a single block.
            let bx = (b.x / CELL) as usize;
            let by = (b.y / CELL) as usize;
            assert_eq!(bx, ((b.x + b.w) / CELL) as usize);
            assert_eq!(by, ((b.y + b.h) / CELL) as usize);
            assert!(b.x >= bx as f64 * CELL + ROAD - 1e-6);
            assert!(b.y >= by as f64 * CELL + ROAD - 1e-6);
            assert!(b.x + b.w <= bx as f64 * CELL + ROAD + BLOCK + 1e-6);
            assert!(b.y + b.h <= by as f64 * CELL + ROAD + BLOCK + 1e-6);
        }
    }

    #[test]
    fn circle_collision_pushes_out_of_building() {
        let city = City::new(1);
        // Find a building and try to push a circle into it.
        let target = city.buildings().next().expect("at least one building");
        let (cx, cy) = target.center();
        // A circle centered in the building must be resolved outside it.
        let mut x = cx;
        let mut y = cy;
        for _ in 0..8 {
            if let Some((nx, ny, _, _)) = city.collide_buildings(x, y, 10.0) {
                x = nx;
                y = ny;
            } else {
                break;
            }
        }
        assert!(!target.contains(x, y), "circle must not end inside building");
    }

    #[test]
    fn circle_collision_free_point_unchanged() {
        let city = City::new(1);
        let (x, y) = City::intersection_pos(3, 4);
        assert!(city.collide_buildings(x, y, 30.0).is_none());
    }

    #[test]
    fn random_road_point_respects_min_distance() {
        let city = City::new(3);
        let mut rng = Rng::new(99);
        for _ in 0..50 {
            let (x, y) = city.random_road_point(&mut rng, SIZE / 2.0, SIZE / 2.0, 700.0);
            let d = ((x - SIZE / 2.0).powi(2) + (y - SIZE / 2.0).powi(2)).sqrt();
            assert!(city.is_road(x, y), "({x},{y}) must be on a road");
            assert!(d >= 700.0 - 1e-6, "min_dist violated: {d}");
        }
    }

    #[test]
    fn generation_is_deterministic_for_a_seed() {
        let a = City::new(1234);
        let b = City::new(1234);
        let ba: Vec<(f64, f64, f64, f64)> = a.buildings().map(|b| (b.x, b.y, b.w, b.h)).collect();
        let bb: Vec<(f64, f64, f64, f64)> = b.buildings().map(|b| (b.x, b.y, b.w, b.h)).collect();
        assert_eq!(ba, bb);
    }
}
