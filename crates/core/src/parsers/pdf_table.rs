//! Pure Rust visual ruling-line and intersection table extraction algorithm.
//!
//! Ported and adapted from `pdfplumber` (`pdfplumber.table`):
//! - Line segment extraction, normalization, and snapping (`snap_edges`).
//! - Collinear line segment joining (`join_edge_group` / `merge_edges`).
//! - Grid vertex intersection detection (`find_intersections`).
//! - Closed cell cycle polygon reconstruction (`intersections_to_cells`).
//! - Contiguous cell clustering into tables (`cells_to_tables`).
//! - Spatial word-to-cell assignment (`cluster_words_into_cells`).
//! - Imaginary gutter edge synthesis (`words_to_edges_v` / `words_to_edges_h`).
//! - GFM Markdown table emission with pipe escaping and boundary alignment.

use std::collections::BTreeMap;

/// Orientation of an axis-aligned edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeOrientation {
    Horizontal,
    Vertical,
}

/// An axis-aligned edge segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub orientation: EdgeOrientation,
    pub x0: f32,
    pub x1: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Edge {
    /// Create a horizontal edge at vertical position `y` spanning `[x0, x1]`.
    pub fn horizontal(x0: f32, x1: f32, y: f32) -> Self {
        let (min_x, max_x) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        Self {
            orientation: EdgeOrientation::Horizontal,
            x0: min_x,
            x1: max_x,
            top: y,
            bottom: y,
        }
    }

    /// Create a vertical edge at horizontal position `x` spanning `[y0, y1]`.
    pub fn vertical(x: f32, y0: f32, y1: f32) -> Self {
        let (min_y, max_y) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        Self {
            orientation: EdgeOrientation::Vertical,
            x0: x,
            x1: x,
            top: min_y,
            bottom: max_y,
        }
    }

    /// Length of the edge segment in points.
    pub fn length(&self) -> f32 {
        match self.orientation {
            EdgeOrientation::Horizontal => self.x1 - self.x0,
            EdgeOrientation::Vertical => self.bottom - self.top,
        }
    }
}

/// A word extracted from a PDF page with spatial coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfWord {
    pub text: String,
    pub x0: f32,
    pub top: f32,
    pub x1: f32,
    pub bottom: f32,
}

impl PdfWord {
    pub fn new(text: impl Into<String>, x0: f32, top: f32, x1: f32, bottom: f32) -> Self {
        Self {
            text: text.into(),
            x0: x0.min(x1),
            x1: x0.max(x1),
            top: top.min(bottom),
            bottom: top.max(bottom),
        }
    }

    pub fn midpoint(&self) -> (f32, f32) {
        ((self.x0 + self.x1) * 0.5, (self.top + self.bottom) * 0.5)
    }
}

/// Configuration settings for the table extraction algorithm.
#[derive(Debug, Clone)]
pub struct TableSettings {
    /// Snap tolerance in points (default: 3.0). Nearly touching edges snap to their average.
    pub snap_tolerance: f32,
    /// Join tolerance in points (default: 3.0). Collinear edges within this gap are merged.
    pub join_tolerance: f32,
    /// Intersection tolerance in points (default: 3.0).
    pub intersection_tolerance: f32,
    /// Minimum edge length in points to consider (default: 3.0).
    pub edge_min_length: f32,
    /// Minimum aligned words required to synthesize a vertical gutter edge (default: 3).
    pub min_words_vertical: usize,
    /// Minimum aligned words required to synthesize a horizontal edge (default: 1).
    pub min_words_horizontal: usize,
}

impl Default for TableSettings {
    fn default() -> Self {
        Self {
            snap_tolerance: 3.0,
            join_tolerance: 3.0,
            intersection_tolerance: 3.0,
            edge_min_length: 3.0,
            min_words_vertical: 3,
            min_words_horizontal: 1,
        }
    }
}

/// Snaps edges within `snap_tolerance` to their positional average.
pub fn snap_edges(edges: &[Edge], snap_x_tol: f32, snap_y_tol: f32) -> Vec<Edge> {
    let mut v_edges: Vec<Edge> = edges
        .iter()
        .filter(|e| e.orientation == EdgeOrientation::Vertical)
        .cloned()
        .collect();
    let mut h_edges: Vec<Edge> = edges
        .iter()
        .filter(|e| e.orientation == EdgeOrientation::Horizontal)
        .cloned()
        .collect();

    // Snap vertical edges along X
    if snap_x_tol > 0.0 && !v_edges.is_empty() {
        v_edges.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
        let mut clusters: Vec<Vec<Edge>> = Vec::new();
        let mut current_cluster = vec![v_edges[0]];
        let mut last_x = v_edges[0].x0;

        for e in v_edges.into_iter().skip(1) {
            if e.x0 <= last_x + snap_x_tol {
                current_cluster.push(e);
            } else {
                clusters.push(current_cluster);
                current_cluster = vec![e];
            }
            last_x = e.x0;
        }
        clusters.push(current_cluster);

        let mut snapped_v = Vec::new();
        for cluster in clusters {
            let avg_x: f32 = cluster.iter().map(|e| e.x0).sum::<f32>() / (cluster.len() as f32);
            for mut e in cluster {
                e.x0 = avg_x;
                e.x1 = avg_x;
                snapped_v.push(e);
            }
        }
        v_edges = snapped_v;
    }

    // Snap horizontal edges along Y (top)
    if snap_y_tol > 0.0 && !h_edges.is_empty() {
        h_edges.sort_by(|a, b| {
            a.top
                .partial_cmp(&b.top)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut clusters: Vec<Vec<Edge>> = Vec::new();
        let mut current_cluster = vec![h_edges[0]];
        let mut last_y = h_edges[0].top;

        for e in h_edges.into_iter().skip(1) {
            if e.top <= last_y + snap_y_tol {
                current_cluster.push(e);
            } else {
                clusters.push(current_cluster);
                current_cluster = vec![e];
            }
            last_y = e.top;
        }
        clusters.push(current_cluster);

        let mut snapped_h = Vec::new();
        for cluster in clusters {
            let avg_y: f32 = cluster.iter().map(|e| e.top).sum::<f32>() / (cluster.len() as f32);
            for mut e in cluster {
                e.top = avg_y;
                e.bottom = avg_y;
                snapped_h.push(e);
            }
        }
        h_edges = snapped_h;
    }

    let mut result = v_edges;
    result.extend(h_edges);
    result
}

/// Joins collinear edges along the same infinite line that are within `tolerance` of one another.
pub fn join_edge_group(
    mut edges: Vec<Edge>,
    orientation: EdgeOrientation,
    tolerance: f32,
) -> Vec<Edge> {
    if edges.is_empty() {
        return edges;
    }
    match orientation {
        EdgeOrientation::Horizontal => {
            edges.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
            let mut joined = vec![edges[0]];
            for e in edges.into_iter().skip(1) {
                let last = joined.last_mut().unwrap();
                if e.x0 <= last.x1 + tolerance {
                    if e.x1 > last.x1 {
                        last.x1 = e.x1;
                    }
                } else {
                    joined.push(e);
                }
            }
            joined
        }
        EdgeOrientation::Vertical => {
            edges.sort_by(|a, b| {
                a.top
                    .partial_cmp(&b.top)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut joined = vec![edges[0]];
            for e in edges.into_iter().skip(1) {
                let last = joined.last_mut().unwrap();
                if e.top <= last.bottom + tolerance {
                    if e.bottom > last.bottom {
                        last.bottom = e.bottom;
                    }
                } else {
                    joined.push(e);
                }
            }
            joined
        }
    }
}

/// Normalizes, snaps, and merges edges into continuous ruling lines.
pub fn merge_edges(edges: &[Edge], settings: &TableSettings) -> Vec<Edge> {
    let filtered: Vec<Edge> = edges
        .iter()
        .filter(|e| e.length() >= settings.edge_min_length)
        .copied()
        .collect();

    let snapped = snap_edges(&filtered, settings.snap_tolerance, settings.snap_tolerance);

    // Group horizontal edges by proximity within snap_tolerance
    let mut h_clusters: Vec<(f32, Vec<Edge>)> = Vec::new();
    for e in snapped
        .iter()
        .filter(|e| e.orientation == EdgeOrientation::Horizontal)
    {
        if let Some((avg_y, group)) = h_clusters
            .iter_mut()
            .find(|(y, _)| (e.top - *y).abs() <= settings.snap_tolerance)
        {
            *avg_y = (*avg_y * group.len() as f32 + e.top) / (group.len() + 1) as f32;
            group.push(*e);
        } else {
            h_clusters.push((e.top, vec![*e]));
        }
    }

    // Group vertical edges by proximity within snap_tolerance
    let mut v_clusters: Vec<(f32, Vec<Edge>)> = Vec::new();
    for e in snapped
        .iter()
        .filter(|e| e.orientation == EdgeOrientation::Vertical)
    {
        if let Some((avg_x, group)) = v_clusters
            .iter_mut()
            .find(|(x, _)| (e.x0 - *x).abs() <= settings.snap_tolerance)
        {
            *avg_x = (*avg_x * group.len() as f32 + e.x0) / (group.len() + 1) as f32;
            group.push(*e);
        } else {
            v_clusters.push((e.x0, vec![*e]));
        }
    }

    let mut result = Vec::new();
    for (avg_y, mut group) in h_clusters {
        for e in &mut group {
            e.top = avg_y;
            e.bottom = avg_y;
        }
        let joined = join_edge_group(group, EdgeOrientation::Horizontal, settings.join_tolerance);
        result.extend(joined);
    }
    for (avg_x, mut group) in v_clusters {
        for e in &mut group {
            e.x0 = avg_x;
            e.x1 = avg_x;
        }
        let joined = join_edge_group(group, EdgeOrientation::Vertical, settings.join_tolerance);
        result.extend(joined);
    }

    result
}

/// An intersection point between vertical and horizontal edges.
#[derive(Debug, Clone)]
pub struct IntersectionPoint {
    pub x: f32,
    pub y: f32,
    pub v_edges: Vec<Edge>,
    pub h_edges: Vec<Edge>,
}

/// Finds intersection points $(x, y)$ between vertical and horizontal edges.
pub fn find_intersections(
    edges: &[Edge],
    x_tolerance: f32,
    y_tolerance: f32,
) -> BTreeMap<(i32, i32), IntersectionPoint> {
    let v_edges: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.orientation == EdgeOrientation::Vertical)
        .collect();
    let h_edges: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.orientation == EdgeOrientation::Horizontal)
        .collect();

    let mut map: BTreeMap<(i32, i32), IntersectionPoint> = BTreeMap::new();

    for v in &v_edges {
        for h in &h_edges {
            if v.top <= h.top + y_tolerance
                && v.bottom >= h.top - y_tolerance
                && v.x0 >= h.x0 - x_tolerance
                && v.x0 <= h.x1 + x_tolerance
            {
                let x = v.x0;
                let y = h.top;
                let key = ((x * 10.0).round() as i32, (y * 10.0).round() as i32);

                let entry = map.entry(key).or_insert_with(|| IntersectionPoint {
                    x,
                    y,
                    v_edges: Vec::new(),
                    h_edges: Vec::new(),
                });
                if !entry.v_edges.iter().any(|e| e == *v) {
                    entry.v_edges.push(**v);
                }
                if !entry.h_edges.iter().any(|e| e == *h) {
                    entry.h_edges.push(**h);
                }
            }
        }
    }

    map
}

/// A bounding box representing a table cell: `[x0, top, x1, bottom]`.
#[derive(Debug, Clone, PartialEq)]
pub struct CellBbox {
    pub x0: f32,
    pub top: f32,
    pub x1: f32,
    pub bottom: f32,
}

impl CellBbox {
    pub fn new(x0: f32, top: f32, x1: f32, bottom: f32) -> Self {
        Self {
            x0: x0.min(x1),
            top: top.min(bottom),
            x1: x0.max(x1),
            bottom: top.max(bottom),
        }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    pub fn contains_point(&self, x: f32, y: f32, tolerance: f32) -> bool {
        x >= self.x0 - tolerance
            && x <= self.x1 + tolerance
            && y >= self.top - tolerance
            && y <= self.bottom + tolerance
    }
}

/// Reconstructs rectangular cells from intersection vertices.
pub fn intersections_to_cells(
    intersections: &BTreeMap<(i32, i32), IntersectionPoint>,
    tolerance: f32,
) -> Vec<CellBbox> {
    let mut points: Vec<&IntersectionPoint> = intersections.values().collect();
    // Sort top-to-bottom, then left-to-right
    points.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let connects_vertically = |p1: &IntersectionPoint, p2: &IntersectionPoint, tol: f32| -> bool {
        let (min_y, max_y) = if p1.y <= p2.y {
            (p1.y, p2.y)
        } else {
            (p2.y, p1.y)
        };
        p1.v_edges.iter().any(|v| {
            p2.v_edges.iter().any(|v2| {
                (v.x0 - v2.x0).abs() <= tol && v.top <= min_y + tol && v.bottom >= max_y - tol
            })
        })
    };

    let connects_horizontally =
        |p1: &IntersectionPoint, p2: &IntersectionPoint, tol: f32| -> bool {
            let (min_x, max_x) = if p1.x <= p2.x {
                (p1.x, p2.x)
            } else {
                (p2.x, p1.x)
            };
            p1.h_edges.iter().any(|h| {
                p2.h_edges.iter().any(|h2| {
                    (h.top - h2.top).abs() <= tol && h.x0 <= min_x + tol && h.x1 >= max_x - tol
                })
            })
        };

    let mut cells = Vec::new();
    let n = points.len();

    for i in 0..n {
        let p1 = points[i];
        let rest = &points[i + 1..];

        // Candidate points directly below p1 (same x within tolerance, y > p1.y)
        let mut below: Vec<&&IntersectionPoint> = rest
            .iter()
            .filter(|p| (p.x - p1.x).abs() <= tolerance && p.y > p1.y + tolerance)
            .collect();
        below.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));

        // Candidate points directly right of p1 (same y within tolerance, x > p1.x)
        let mut right: Vec<&&IntersectionPoint> = rest
            .iter()
            .filter(|p| (p.y - p1.y).abs() <= tolerance && p.x > p1.x + tolerance)
            .collect();
        right.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));

        let mut found_cell = false;
        for p2 in below {
            if !connects_vertically(p1, p2, tolerance) {
                continue;
            }

            for p3 in &right {
                if !connects_horizontally(p1, p3, tolerance) {
                    continue;
                }

                // Check for p4 at (p3.x, p2.y)
                let p4_target_x = p3.x;
                let p4_target_y = p2.y;

                if let Some(p4) = points.iter().find(|p| {
                    (p.x - p4_target_x).abs() <= tolerance && (p.y - p4_target_y).abs() <= tolerance
                }) {
                    if connects_vertically(p3, p4, tolerance)
                        && connects_horizontally(p2, p4, tolerance)
                    {
                        // Ensure no intermediate intersection points cut strictly inside this cell
                        let has_inner_split = points.iter().any(|inner| {
                            inner.x > p1.x + tolerance
                                && inner.x < p3.x - tolerance
                                && inner.y > p1.y + tolerance
                                && inner.y < p2.y - tolerance
                        });

                        if !has_inner_split {
                            cells.push(CellBbox::new(p1.x, p1.y, p3.x, p2.y));
                            found_cell = true;
                            break;
                        }
                    }
                }
            }
            if found_cell {
                break;
            }
        }
    }

    cells
}

/// Clusters cells into contiguous tables.
pub fn cells_to_tables(cells: &[CellBbox], tolerance: f32) -> Vec<Vec<CellBbox>> {
    if cells.is_empty() {
        return Vec::new();
    }

    let mut remaining = cells.to_vec();
    let mut tables: Vec<Vec<CellBbox>> = Vec::new();

    while !remaining.is_empty() {
        let mut current_table = vec![remaining.remove(0)];
        let mut i = 0;

        while i < current_table.len() {
            let active = current_table[i].clone();
            let mut j = 0;
            while j < remaining.len() {
                let candidate = &remaining[j];
                // Check if candidate touches active cell on any corner or edge
                let touches_x = (candidate.x0 - active.x1).abs() <= tolerance
                    || (candidate.x1 - active.x0).abs() <= tolerance
                    || (candidate.x0 >= active.x0 - tolerance
                        && candidate.x0 <= active.x1 + tolerance)
                    || (active.x0 >= candidate.x0 - tolerance
                        && active.x0 <= candidate.x1 + tolerance);

                let touches_y = (candidate.top - active.bottom).abs() <= tolerance
                    || (candidate.bottom - active.top).abs() <= tolerance
                    || (candidate.top >= active.top - tolerance
                        && candidate.top <= active.bottom + tolerance)
                    || (active.top >= candidate.top - tolerance
                        && active.top <= candidate.bottom + tolerance);

                if touches_x && touches_y {
                    current_table.push(remaining.remove(j));
                } else {
                    j += 1;
                }
            }
            i += 1;
        }

        tables.push(current_table);
    }

    // Sort tables by top coordinate, then left
    tables.sort_by(|t1, t2| {
        let min_y1 = t1.iter().map(|c| c.top).fold(f32::INFINITY, f32::min);
        let min_y2 = t2.iter().map(|c| c.top).fold(f32::INFINITY, f32::min);
        min_y1
            .partial_cmp(&min_y2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let min_x1 = t1.iter().map(|c| c.x0).fold(f32::INFINITY, f32::min);
                let min_x2 = t2.iter().map(|c| c.x0).fold(f32::INFINITY, f32::min);
                min_x1
                    .partial_cmp(&min_x2)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    tables
}

/// A cell populated with extracted text and span information.
#[derive(Debug, Clone)]
pub struct TableCell {
    pub bbox: CellBbox,
    pub text: String,
    pub col_span: usize,
    pub row_span: usize,
}

/// A structured 2D table extracted from ruling lines and words.
#[derive(Debug, Clone)]
pub struct ExtractedTable {
    pub bbox: CellBbox,
    pub rows: Vec<Vec<TableCell>>,
}

impl ExtractedTable {
    /// Renders the table as a clean CommonMark/GFM Markdown pipe table.
    pub fn to_markdown(&self) -> String {
        if self.rows.is_empty() {
            return String::new();
        }

        // Determine maximum number of columns
        let col_count = self
            .rows
            .iter()
            .map(|r| r.iter().map(|c| c.col_span).sum::<usize>())
            .max()
            .unwrap_or(0);

        if col_count == 0 {
            return String::new();
        }

        let mut lines = Vec::new();

        for (row_idx, row) in self.rows.iter().enumerate() {
            let mut row_cells: Vec<String> = Vec::new();
            for cell in row {
                let sanitized = cell
                    .text
                    .chars()
                    .filter(|c| !c.is_control() || *c == ' ' || *c == '\t')
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .replace('|', "\\|");
                row_cells.push(sanitized);
                // If cell has colspan > 1, pad with empty cells
                for _ in 1..cell.col_span {
                    row_cells.push(String::new());
                }
            }
            while row_cells.len() < col_count {
                row_cells.push(String::new());
            }

            lines.push(format!("| {} |", row_cells.join(" | ")));

            // Emit separator row after header (row 0)
            if row_idx == 0 {
                let sep = vec!["---"; col_count];
                lines.push(format!("|{}|", sep.join("|")));
            }
        }

        format!("\n\n{}\n\n", lines.join("\n"))
    }
}

/// Assigns words to cells based on bounding box containment.
pub fn cluster_words_into_cells(
    cells: &[CellBbox],
    words: &[PdfWord],
    tolerance: f32,
) -> Vec<(CellBbox, String)> {
    let mut result = Vec::new();

    for cell in cells {
        let mut cell_words: Vec<&PdfWord> = words
            .iter()
            .filter(|w| {
                let (mid_x, mid_y) = w.midpoint();
                cell.contains_point(mid_x, mid_y, tolerance)
            })
            .collect();

        // Sort words top-to-bottom (grouped into lines within 3pt), then left-to-right
        cell_words.sort_by(|a, b| {
            if (a.top - b.top).abs() <= 3.0 {
                a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                a.top
                    .partial_cmp(&b.top)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        });

        let mut text_lines: Vec<String> = Vec::new();
        let mut current_line: Vec<String> = Vec::new();
        let mut last_top = if let Some(first) = cell_words.first() {
            first.top
        } else {
            0.0
        };

        for w in cell_words {
            if (w.top - last_top).abs() > 4.0 && !current_line.is_empty() {
                text_lines.push(current_line.join(" "));
                current_line.clear();
            }
            current_line.push(w.text.clone());
            last_top = w.top;
        }
        if !current_line.is_empty() {
            text_lines.push(current_line.join(" "));
        }

        let full_text = text_lines.join(" ");
        result.push((cell.clone(), full_text));
    }

    result
}

/// Builds an `ExtractedTable` by organizing cells and words into a 2D grid.
pub fn build_table_from_cells_and_words(
    cells: &[CellBbox],
    words: &[PdfWord],
    tolerance: f32,
) -> Option<ExtractedTable> {
    if cells.is_empty() {
        return None;
    }

    let cell_contents = cluster_words_into_cells(cells, words, tolerance);

    // Compute unique row boundaries (sorted Y coordinates)
    let mut y_coords: Vec<f32> = cells.iter().flat_map(|c| [c.top, c.bottom]).collect();
    y_coords.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    y_coords.dedup_by(|a, b| (*a - *b).abs() <= tolerance);

    // Compute unique column boundaries (sorted X coordinates)
    let mut x_coords: Vec<f32> = cells.iter().flat_map(|c| [c.x0, c.x1]).collect();
    x_coords.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    x_coords.dedup_by(|a, b| (*a - *b).abs() <= tolerance);

    if y_coords.len() < 2 || x_coords.len() < 2 {
        return None;
    }

    let min_x = x_coords[0];
    let max_x = *x_coords.last().unwrap();
    let min_y = y_coords[0];
    let max_y = *y_coords.last().unwrap();
    let table_bbox = CellBbox::new(min_x, min_y, max_x, max_y);

    // Group cells into rows based on top coordinate
    let mut row_groups: BTreeMap<i32, Vec<(CellBbox, String)>> = BTreeMap::new();
    for (cell, text) in cell_contents {
        let key = (cell.top * 10.0).round() as i32;
        // Find existing row key within tolerance
        let matched_key = row_groups
            .keys()
            .find(|&&k| (k as f32 / 10.0 - cell.top).abs() <= tolerance)
            .copied();

        if let Some(mk) = matched_key {
            row_groups.get_mut(&mk).unwrap().push((cell, text));
        } else {
            row_groups.insert(key, vec![(cell, text)]);
        }
    }

    let mut rows = Vec::new();
    for (_, mut row_cells) in row_groups {
        row_cells
            .sort_by(|(a, _), (b, _)| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));

        let mut row_table_cells = Vec::new();
        for (cell, text) in row_cells {
            // Calculate column span
            let col_start = x_coords
                .iter()
                .position(|&x| (x - cell.x0).abs() <= tolerance)
                .unwrap_or(0);
            let col_end = x_coords
                .iter()
                .position(|&x| (x - cell.x1).abs() <= tolerance)
                .unwrap_or(col_start + 1);
            let col_span = (col_end - col_start).max(1);

            row_table_cells.push(TableCell {
                bbox: cell,
                text,
                col_span,
                row_span: 1,
            });
        }
        rows.push(row_table_cells);
    }

    Some(ExtractedTable {
        bbox: table_bbox,
        rows,
    })
}

/// Synthesizes imaginary vertical column boundary edges from aligned words (gutter detection).
pub fn words_to_edges_v(words: &[PdfWord], min_words_vertical: usize) -> Vec<Edge> {
    if words.is_empty() {
        return Vec::new();
    }
    let min_top = words.iter().map(|w| w.top).fold(f32::INFINITY, f32::min);
    let max_bottom = words
        .iter()
        .map(|w| w.bottom)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_x0 = words.iter().map(|w| w.x0).fold(f32::INFINITY, f32::min);
    let max_x1 = words.iter().map(|w| w.x1).fold(f32::NEG_INFINITY, f32::max);

    words_to_edges_v_bounded(
        words,
        min_words_vertical,
        min_top,
        max_bottom,
        min_x0,
        max_x1,
    )
}

/// Synthesizes vertical column boundary edges bounded by given bounds.
pub fn words_to_edges_v_bounded(
    words: &[PdfWord],
    min_words_vertical: usize,
    min_top: f32,
    max_bottom: f32,
    min_x0: f32,
    max_x1: f32,
) -> Vec<Edge> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut by_x0: BTreeMap<i32, Vec<&PdfWord>> = BTreeMap::new();
    for w in words {
        let key = (w.x0 * 0.25).round() as i32; // Cluster within ~4pt
        by_x0.entry(key).or_default().push(w);
    }

    struct ColumnInfo {
        min_x0: f32,
        max_x1: f32,
    }

    let mut columns: Vec<ColumnInfo> = Vec::new();
    for (_, cluster) in by_x0 {
        if cluster.len() >= min_words_vertical {
            let col_min = cluster.iter().map(|w| w.x0).fold(f32::INFINITY, f32::min);
            let col_max = cluster
                .iter()
                .map(|w| w.x1)
                .fold(f32::NEG_INFINITY, f32::max);
            columns.push(ColumnInfo {
                min_x0: col_min,
                max_x1: col_max,
            });
        }
    }

    columns.sort_by(|a, b| {
        a.min_x0
            .partial_cmp(&b.min_x0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if columns.is_empty() {
        return Vec::new();
    }

    let mut edges = Vec::new();
    // Outer left edge
    edges.push(Edge::vertical(min_x0, min_top, max_bottom));

    // Dividing edges between columns
    for i in 0..columns.len().saturating_sub(1) {
        let div_x = (columns[i].max_x1 + columns[i + 1].min_x0) * 0.5;
        edges.push(Edge::vertical(div_x, min_top, max_bottom));
    }

    // Outer right edge
    edges.push(Edge::vertical(max_x1, min_top, max_bottom));

    edges
}

/// Synthesizes imaginary horizontal row boundary edges connecting word tops and bottoms.
pub fn words_to_edges_h(words: &[PdfWord], min_words_horizontal: usize) -> Vec<Edge> {
    if words.is_empty() {
        return Vec::new();
    }
    let min_top = words.iter().map(|w| w.top).fold(f32::INFINITY, f32::min);
    let max_bottom = words
        .iter()
        .map(|w| w.bottom)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_x0 = words.iter().map(|w| w.x0).fold(f32::INFINITY, f32::min);
    let max_x1 = words.iter().map(|w| w.x1).fold(f32::NEG_INFINITY, f32::max);

    words_to_edges_h_bounded(
        words,
        min_words_horizontal,
        min_top,
        max_bottom,
        min_x0,
        max_x1,
    )
}

/// Synthesizes horizontal row boundary edges bounded by given bounds.
pub fn words_to_edges_h_bounded(
    words: &[PdfWord],
    min_words_horizontal: usize,
    min_top: f32,
    max_bottom: f32,
    min_x0: f32,
    max_x1: f32,
) -> Vec<Edge> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut by_top: BTreeMap<i32, Vec<&PdfWord>> = BTreeMap::new();
    for w in words {
        let key = (w.top * 0.25).round() as i32; // Cluster within ~4pt
        by_top.entry(key).or_default().push(w);
    }

    struct RowInfo {
        min_top: f32,
        max_bottom: f32,
    }

    let mut rows: Vec<RowInfo> = Vec::new();
    for (_, cluster) in by_top {
        if cluster.len() >= min_words_horizontal {
            let row_top = cluster.iter().map(|w| w.top).fold(f32::INFINITY, f32::min);
            let row_bottom = cluster
                .iter()
                .map(|w| w.bottom)
                .fold(f32::NEG_INFINITY, f32::max);
            rows.push(RowInfo {
                min_top: row_top,
                max_bottom: row_bottom,
            });
        }
    }

    rows.sort_by(|a, b| {
        a.min_top
            .partial_cmp(&b.min_top)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if rows.is_empty() {
        return Vec::new();
    }

    let mut edges = Vec::new();
    // Top border
    edges.push(Edge::horizontal(min_x0, max_x1, min_top));

    // Dividers between rows
    for i in 0..rows.len().saturating_sub(1) {
        let div_y = (rows[i].max_bottom + rows[i + 1].min_top) * 0.5;
        edges.push(Edge::horizontal(min_x0, max_x1, div_y));
    }

    // Bottom border
    edges.push(Edge::horizontal(min_x0, max_x1, max_bottom));

    edges
}

/// Main end-to-end table extraction function from ruling lines and words.
pub fn extract_tables_from_edges_and_words(
    edges: &[Edge],
    words: &[PdfWord],
    settings: &TableSettings,
) -> Vec<ExtractedTable> {
    let mut all_edges = edges.to_vec();

    // If vertical lines are missing, fall back to word-aligned gutters
    let has_v = all_edges
        .iter()
        .any(|e| e.orientation == EdgeOrientation::Vertical);
    if !has_v && !words.is_empty() {
        let min_top = all_edges
            .iter()
            .map(|e| e.top)
            .fold(f32::INFINITY, f32::min)
            .min(words.iter().map(|w| w.top).fold(f32::INFINITY, f32::min));
        let max_bottom = all_edges
            .iter()
            .map(|e| e.bottom)
            .fold(f32::NEG_INFINITY, f32::max)
            .max(
                words
                    .iter()
                    .map(|w| w.bottom)
                    .fold(f32::NEG_INFINITY, f32::max),
            );
        let min_x0 = all_edges
            .iter()
            .map(|e| e.x0)
            .fold(f32::INFINITY, f32::min)
            .min(words.iter().map(|w| w.x0).fold(f32::INFINITY, f32::min));
        let max_x1 = all_edges
            .iter()
            .map(|e| e.x1)
            .fold(f32::NEG_INFINITY, f32::max)
            .max(words.iter().map(|w| w.x1).fold(f32::NEG_INFINITY, f32::max));

        let gutter_edges = words_to_edges_v_bounded(
            words,
            settings.min_words_vertical,
            min_top,
            max_bottom,
            min_x0,
            max_x1,
        );
        all_edges.extend(gutter_edges);
    }

    // If horizontal lines are missing, fall back to word line edges
    let has_h = all_edges
        .iter()
        .any(|e| e.orientation == EdgeOrientation::Horizontal);
    if !has_h && !words.is_empty() {
        let min_top = all_edges
            .iter()
            .map(|e| e.top)
            .fold(f32::INFINITY, f32::min)
            .min(words.iter().map(|w| w.top).fold(f32::INFINITY, f32::min));
        let max_bottom = all_edges
            .iter()
            .map(|e| e.bottom)
            .fold(f32::NEG_INFINITY, f32::max)
            .max(
                words
                    .iter()
                    .map(|w| w.bottom)
                    .fold(f32::NEG_INFINITY, f32::max),
            );
        let min_x0 = all_edges
            .iter()
            .map(|e| e.x0)
            .fold(f32::INFINITY, f32::min)
            .min(words.iter().map(|w| w.x0).fold(f32::INFINITY, f32::min));
        let max_x1 = all_edges
            .iter()
            .map(|e| e.x1)
            .fold(f32::NEG_INFINITY, f32::max)
            .max(words.iter().map(|w| w.x1).fold(f32::NEG_INFINITY, f32::max));

        let row_edges = words_to_edges_h_bounded(
            words,
            settings.min_words_horizontal,
            min_top,
            max_bottom,
            min_x0,
            max_x1,
        );
        all_edges.extend(row_edges);
    }

    let merged = merge_edges(&all_edges, settings);
    let intersections = find_intersections(
        &merged,
        settings.intersection_tolerance,
        settings.intersection_tolerance,
    );
    let cells = intersections_to_cells(&intersections, settings.intersection_tolerance);
    let table_cells = cells_to_tables(&cells, settings.snap_tolerance);

    let mut tables = Vec::new();
    for table_group in table_cells {
        if let Some(table) =
            build_table_from_cells_and_words(&table_group, words, settings.intersection_tolerance)
        {
            // Require at least 2 cells to qualify as a valid table
            let total_cells: usize = table.rows.iter().map(|r| r.len()).sum();
            if total_cells >= 2 {
                tables.push(table);
            }
        }
    }

    tables
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_and_snap_edges() {
        let settings = TableSettings::default();
        // Two nearly touching collinear horizontal edges with a small gap
        let e1 = Edge::horizontal(10.0, 50.0, 100.0);
        let e2 = Edge::horizontal(52.0, 90.0, 100.8); // within snap and join tolerance
        let merged = merge_edges(&[e1, e2], &settings);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].orientation, EdgeOrientation::Horizontal);
        assert_eq!(merged[0].x0, 10.0);
        assert_eq!(merged[0].x1, 90.0);
    }

    #[test]
    fn test_intersections_and_cells() {
        let settings = TableSettings::default();
        // 2x2 grid formed by 3 vertical and 3 horizontal lines:
        // x in [0, 50, 100], y in [0, 50, 100]
        let edges = vec![
            Edge::horizontal(0.0, 100.0, 0.0),
            Edge::horizontal(0.0, 100.0, 50.0),
            Edge::horizontal(0.0, 100.0, 100.0),
            Edge::vertical(0.0, 0.0, 100.0),
            Edge::vertical(50.0, 0.0, 100.0),
            Edge::vertical(100.0, 0.0, 100.0),
        ];

        let merged = merge_edges(&edges, &settings);
        let intersections = find_intersections(&merged, 3.0, 3.0);
        assert_eq!(intersections.len(), 9);

        let cells = intersections_to_cells(&intersections, 3.0);
        assert_eq!(
            cells.len(),
            4,
            "A 2x2 grid must produce exactly 4 atomic cells"
        );
    }

    #[test]
    fn test_merged_header_colspan() {
        let settings = TableSettings::default();
        // A table where row 0 is a single header spanning 0..100 (colspan 2),
        // and row 1 is split into two columns: 0..50 and 50..100.
        let edges = vec![
            Edge::horizontal(0.0, 100.0, 0.0),  // top border
            Edge::horizontal(0.0, 100.0, 30.0), // middle divider
            Edge::horizontal(0.0, 100.0, 60.0), // bottom border
            Edge::vertical(0.0, 0.0, 60.0),     // left border
            Edge::vertical(50.0, 30.0, 60.0),   // inner column divider ONLY in row 1
            Edge::vertical(100.0, 0.0, 60.0),   // right border
        ];

        let words = vec![
            PdfWord::new("Quarterly Revenue", 20.0, 10.0, 80.0, 20.0),
            PdfWord::new("Q1", 10.0, 40.0, 30.0, 50.0),
            PdfWord::new("$50M", 60.0, 40.0, 90.0, 50.0),
        ];

        let tables = extract_tables_from_edges_and_words(&edges, &words, &settings);
        assert_eq!(tables.len(), 1);

        let md = tables[0].to_markdown();
        assert!(md.contains("| Quarterly Revenue |  |"), "{md}");
        assert!(md.contains("|---|---|"), "{md}");
        assert!(md.contains("| Q1 | $50M |"), "{md}");
    }

    #[test]
    fn test_borderless_gutter_fallback() {
        let settings = TableSettings::default();
        // Table with NO vertical edges, only horizontal line dividers
        let edges = vec![
            Edge::horizontal(10.0, 190.0, 100.0),
            Edge::horizontal(10.0, 190.0, 130.0),
            Edge::horizontal(10.0, 190.0, 160.0),
            Edge::horizontal(10.0, 190.0, 190.0),
        ];

        let words = vec![
            // Column 1 (aligned ~20)
            PdfWord::new("Item", 20.0, 110.0, 40.0, 120.0),
            PdfWord::new("Apples", 20.0, 140.0, 50.0, 150.0),
            PdfWord::new("Oranges", 20.0, 170.0, 55.0, 180.0),
            // Column 2 (aligned ~120)
            PdfWord::new("Price", 120.0, 110.0, 140.0, 120.0),
            PdfWord::new("$1.50", 120.0, 140.0, 145.0, 150.0),
            PdfWord::new("$2.00", 120.0, 170.0, 145.0, 180.0),
        ];

        let tables = extract_tables_from_edges_and_words(&edges, &words, &settings);
        assert_eq!(tables.len(), 1);

        let md = tables[0].to_markdown();
        assert!(md.contains("| Item | Price |"), "{md}");
        assert!(md.contains("| Apples | $1.50 |"), "{md}");
        assert!(md.contains("| Oranges | $2.00 |"), "{md}");
    }
}
