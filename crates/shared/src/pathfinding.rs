use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::world::{InsideSimData, InsideVisData, TILE_SIZE, WalkMask};

/// Read-only 1m × 1m grid interface for A* and Line-of-Sight pathfinding.
pub trait WalkGrid {
    fn width(&self) -> usize;
    fn length(&self) -> usize;
    fn is_walkable(&self, x: i32, z: i32) -> bool;
}

impl WalkGrid for WalkMask {
    fn width(&self) -> usize {
        TILE_SIZE
    }

    fn length(&self) -> usize {
        TILE_SIZE
    }

    fn is_walkable(&self, x: i32, z: i32) -> bool {
        if x < 0 || z < 0 {
            return false;
        }
        self.get(x as usize, z as usize)
    }
}

impl WalkGrid for InsideVisData {
    fn width(&self) -> usize {
        self.width as usize
    }

    fn length(&self) -> usize {
        self.length as usize
    }

    fn is_walkable(&self, x: i32, z: i32) -> bool {
        if x < 0 || z < 0 {
            return false;
        }
        self.get_walkable(x as usize, z as usize)
    }
}

impl WalkGrid for InsideSimData {
    fn width(&self) -> usize {
        self.width as usize
    }

    fn length(&self) -> usize {
        self.length as usize
    }

    fn is_walkable(&self, x: i32, z: i32) -> bool {
        if x < 0 || z < 0 {
            return false;
        }
        self.get_walkable(x as usize, z as usize)
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct OpenNode {
    pos: (i32, i32),
    f_score: u32,
}

impl Ord for OpenNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f_score
            .cmp(&self.f_score)
            .then_with(|| other.pos.cmp(&self.pos))
    }
}

impl PartialOrd for OpenNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[inline]
fn octile_heuristic(a: (i32, i32), b: (i32, i32)) -> u32 {
    let dx = (a.0 - b.0).abs() as u32;
    let dz = (a.1 - b.1).abs() as u32;
    let (min_d, max_d) = if dx < dz { (dx, dz) } else { (dz, dx) };
    // Cardinal step = 10, diagonal step = 14
    14 * min_d + 10 * (max_d - min_d)
}

/// Finds an 8-directional shortest path on a 1m grid without cutting corners around obstacles.
pub fn find_path(
    grid: &dyn WalkGrid,
    start: (i32, i32),
    goal: (i32, i32),
) -> Option<Vec<(i32, i32)>> {
    if !grid.is_walkable(start.0, start.1) || !grid.is_walkable(goal.0, goal.1) {
        return None;
    }
    if start == goal {
        return Some(vec![start]);
    }

    let mut open = BinaryHeap::new();
    let mut g_score: HashMap<(i32, i32), u32> = HashMap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();

    g_score.insert(start, 0);
    open.push(OpenNode {
        pos: start,
        f_score: octile_heuristic(start, goal),
    });

    const DIRS: [(i32, i32, u32); 8] = [
        (1, 0, 10),
        (-1, 0, 10),
        (0, 1, 10),
        (0, -1, 10),
        (1, 1, 14),
        (1, -1, 14),
        (-1, 1, 14),
        (-1, -1, 14),
    ];

    while let Some(OpenNode { pos, .. }) = open.pop() {
        if pos == goal {
            let mut path = vec![goal];
            let mut cur = goal;
            while let Some(&prev) = came_from.get(&cur) {
                path.push(prev);
                cur = prev;
                if cur == start {
                    break;
                }
            }
            path.reverse();
            return Some(path);
        }

        let current_g = g_score[&pos];

        for (dx, dz, step_cost) in DIRS {
            let nx = pos.0 + dx;
            let nz = pos.1 + dz;
            if !grid.is_walkable(nx, nz) {
                continue;
            }
            // Prevent diagonal corner cutting across blocked adjacent cells.
            if dx != 0
                && dz != 0
                && (!grid.is_walkable(pos.0 + dx, pos.1) || !grid.is_walkable(pos.0, pos.1 + dz))
            {
                continue;
            }

            let tentative_g = current_g + step_cost;
            if tentative_g < *g_score.get(&(nx, nz)).unwrap_or(&u32::MAX) {
                came_from.insert((nx, nz), pos);
                g_score.insert((nx, nz), tentative_g);
                open.push(OpenNode {
                    pos: (nx, nz),
                    f_score: tentative_g + octile_heuristic((nx, nz), goal),
                });
            }
        }
    }

    None
}

/// Supercover line-of-sight check across 1m cells from `a` to `b`.
pub fn has_line_of_sight(grid: &dyn WalkGrid, a: (i32, i32), b: (i32, i32)) -> bool {
    let mut x = a.0;
    let mut z = a.1;
    let dx = (b.0 - a.0).abs();
    let dz = (b.1 - a.1).abs();
    let sx = (b.0 - a.0).signum();
    let sz = (b.1 - a.1).signum();

    if !grid.is_walkable(x, z) || !grid.is_walkable(b.0, b.1) {
        return false;
    }

    let mut err = dx - dz;
    while x != b.0 || z != b.1 {
        let e2 = 2 * err;
        let step_x = e2 > -dz;
        let step_z = e2 < dx;

        if step_x && step_z {
            // Supercover: require both orthogonal neighbor cells to be clear when crossing diagonally.
            if !grid.is_walkable(x + sx, z) || !grid.is_walkable(x, z + sz) {
                return false;
            }
            err -= dz;
            err += dx;
            x += sx;
            z += sz;
        } else if step_x {
            err -= dz;
            x += sx;
        } else if step_z {
            err += dx;
            z += sz;
        }

        if !grid.is_walkable(x, z) {
            return false;
        }
    }
    true
}

/// Applies Line-of-Sight string-pulling to turn grid steps into direct straight-line waypoints.
pub fn smooth_path(grid: &dyn WalkGrid, path: &[(i32, i32)]) -> Vec<(i32, i32)> {
    if path.len() <= 2 {
        return path.to_vec();
    }

    let mut result = vec![path[0]];
    let mut anchor_idx = 0;

    while anchor_idx < path.len() - 1 {
        let mut furthest = anchor_idx + 1;
        for candidate in (anchor_idx + 2)..path.len() {
            if has_line_of_sight(grid, path[anchor_idx], path[candidate]) {
                furthest = candidate;
            } else {
                break;
            }
        }
        result.push(path[furthest]);
        anchor_idx = furthest;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_smooths_path_around_wall() {
        let mut mask = WalkMask::all_walkable();
        for z in 0..8 {
            mask.set(5, z, false);
        }

        let raw = find_path(&mask, (2, 2), (8, 2)).expect("path should exist around wall");
        assert_eq!(raw.first().copied(), Some((2, 2)));
        assert_eq!(raw.last().copied(), Some((8, 2)));
        assert!(raw.iter().all(|&(x, z)| mask.is_walkable(x, z)));

        let smoothed = smooth_path(&mask, &raw);
        assert!(smoothed.len() <= raw.len());
        assert_eq!(smoothed.first().copied(), Some((2, 2)));
        assert_eq!(smoothed.last().copied(), Some((8, 2)));
    }

    #[test]
    fn returns_none_when_blocked_completely() {
        let mut mask = WalkMask::all_walkable();
        for z in 0..TILE_SIZE {
            mask.set(10, z, false);
        }
        assert!(find_path(&mask, (2, 2), (20, 2)).is_none());
    }
}
