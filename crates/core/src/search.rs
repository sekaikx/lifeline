//! Shortest paths: A* on an 8-connected grid and Dijkstra on a small graph.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Copy, PartialEq)]
struct Entry {
    priority: f64,
    node: usize,
}

impl Eq for Entry {}

impl Ord for Entry {
    // reversed: BinaryHeap is a max-heap and we want the cheapest first
    fn cmp(&self, other: &Self) -> Ordering {
        other.priority.total_cmp(&self.priority)
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub type Cell = (usize, usize);

const NEIGHBOURS: [(i64, i64); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// A* from `start` to `goal` on an `nx` x `ny` grid with 8-neighbour moves.
///
/// `step_cost(from, to)` returns the cost of a move or `None` if it is not
/// allowed. `heuristic(cell)` must never overestimate the remaining cost.
/// Returns the cells from start to goal and the total cost.
pub fn grid_astar(
    nx: usize,
    ny: usize,
    start: Cell,
    goal: Cell,
    mut step_cost: impl FnMut(Cell, Cell) -> Option<f64>,
    heuristic: impl Fn(Cell) -> f64,
) -> Option<(Vec<Cell>, f64)> {
    let idx = |c: Cell| c.1 * nx + c.0;
    let mut best = vec![f64::INFINITY; nx * ny];
    let mut came_from = vec![usize::MAX; nx * ny];
    let mut open = BinaryHeap::new();
    best[idx(start)] = 0.0;
    open.push(Entry {
        priority: heuristic(start),
        node: idx(start),
    });

    while let Some(Entry { priority, node }) = open.pop() {
        let cell = (node % nx, node / nx);
        if cell == goal {
            let mut path = vec![cell];
            let mut n = node;
            while came_from[n] != usize::MAX {
                n = came_from[n];
                path.push((n % nx, n / nx));
            }
            path.reverse();
            return Some((path, best[node]));
        }
        if priority - heuristic(cell) > best[node] + 1e-9 {
            continue; // stale heap entry
        }
        for (di, dj) in NEIGHBOURS {
            let (ni, nj) = (cell.0 as i64 + di, cell.1 as i64 + dj);
            if ni < 0 || nj < 0 || ni >= nx as i64 || nj >= ny as i64 {
                continue;
            }
            let next = (ni as usize, nj as usize);
            let Some(c) = step_cost(cell, next) else {
                continue;
            };
            let g = best[node] + c;
            let k = idx(next);
            if g < best[k] {
                best[k] = g;
                came_from[k] = node;
                open.push(Entry {
                    priority: g + heuristic(next),
                    node: k,
                });
            }
        }
    }
    None
}

/// Dijkstra on a graph given as adjacency lists of `(neighbour, edge id, cost)`.
/// Returns the sequence of edge ids from `start` to `goal`.
pub fn dijkstra(
    adjacency: &[Vec<(usize, usize, f64)>],
    start: usize,
    goal: usize,
) -> Option<(Vec<usize>, f64)> {
    let n = adjacency.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut via: Vec<Option<(usize, usize)>> = vec![None; n];
    let mut open = BinaryHeap::new();
    dist[start] = 0.0;
    open.push(Entry {
        priority: 0.0,
        node: start,
    });
    while let Some(Entry { priority, node }) = open.pop() {
        if node == goal {
            let mut edges = Vec::new();
            let mut v = goal;
            while let Some((prev, edge)) = via[v] {
                edges.push(edge);
                v = prev;
            }
            edges.reverse();
            return Some((edges, dist[goal]));
        }
        if priority > dist[node] {
            continue;
        }
        for &(next, edge, cost) in &adjacency[node] {
            let d = dist[node] + cost;
            if d < dist[next] {
                dist[next] = d;
                via[next] = Some((node, edge));
                open.push(Entry {
                    priority: d,
                    node: next,
                });
            }
        }
    }
    None
}
