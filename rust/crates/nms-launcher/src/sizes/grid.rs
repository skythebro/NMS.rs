//! Port of `grid.py`: columns, rows, window scale, and card size.

use std::collections::BTreeMap;

pub const CARD_W: f64 = 112.0;
pub const CARD_H: f64 = 131.0;
pub const GAP: f64 = 20.0;
pub const AREA_W: f64 = 540.0;
pub const AREA_H: f64 = 280.0;
pub const MIN_CARD: f64 = 0.5;
pub const MAX_ITEMS: i64 = 99;
pub const WINDOW_SCALE_RANGE: (f64, f64) = (1.0, 1.4);

/// Same keys as Python `DEFAULTS`.
pub fn defaults() -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    m.insert("columns", "8".into());
    m.insert("rows", "auto".into());
    m.insert("window_scale", "1.4".into());
    m.insert("inventory_scale", "1.0".into());
    m.insert("compact_tooltips", "true".into());
    m.insert("quick_transfer_all_key", "shift".into());
    m.insert("mining_scales_with_size", "false".into());
    m.insert("craft_from_storage", "false".into());
    m.insert("craft_from_storage_anywhere", "false".into());
    m.insert("write_logs", "true".into());
    m
}

pub const DEFAULT_COLUMNS: i64 = 8;
pub const DEFAULT_WINDOW_SCALE: f64 = 1.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowsSpec {
    Auto,
    Count(i64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub columns: i64,
    pub rows: i64,
    pub window_scale: f64,
}

impl Grid {
    pub fn new(columns: i64, rows: i64, window_scale: f64) -> Self {
        Self {
            columns,
            rows,
            window_scale,
        }
    }

    pub fn items(&self) -> i64 {
        self.columns * self.rows
    }

    pub fn vanilla(&self) -> bool {
        (self.columns, self.rows, self.window_scale) == (4, 2, 1.0)
    }

    pub fn area(&self) -> (f64, f64) {
        (
            AREA_W * self.window_scale,
            AREA_H * self.window_scale,
        )
    }

    pub fn fit_width(&self) -> f64 {
        (self.area().0 / self.columns as f64 - GAP) / CARD_W
    }

    pub fn fit_height(&self, rows: i64) -> f64 {
        (self.area().1 / rows as f64 - GAP) / CARD_H
    }

    pub fn card_size(&self) -> (f64, f64) {
        let scale = self
            .fit_width()
            .min(self.fit_height(self.rows))
            .min(self.window_scale);
        (
            (CARD_W * scale * 10.0).floor() / 10.0,
            (CARD_H * scale * 10.0).floor() / 10.0,
        )
    }

    pub fn extra_size(&self) -> (f64, f64) {
        let (width, height) = self.area();
        (width - AREA_W, height - AREA_H)
    }
}

pub fn auto_rows(columns: i64, window_scale: f64) -> i64 {
    let probe = Grid::new(columns, 1, window_scale);
    let floor = probe.fit_width().min(MIN_CARD * window_scale);
    let mut rows = 1i64;
    while rows < MAX_ITEMS / columns
        && Grid::new(columns, rows + 1, window_scale).fit_height(rows + 1) >= floor
    {
        rows += 1;
    }
    rows
}

pub fn grid_from(
    columns: Option<i64>,
    rows: Option<RowsSpec>,
    window_scale: Option<f64>,
) -> Result<Grid, String> {
    let columns = columns.unwrap_or(DEFAULT_COLUMNS);
    let window_scale = window_scale.unwrap_or(DEFAULT_WINDOW_SCALE);
    let (low, high) = WINDOW_SCALE_RANGE;
    if columns < 1 || columns > MAX_ITEMS {
        return Err(format!("columns must be between 1 and {MAX_ITEMS}"));
    }
    if !(low <= window_scale && window_scale <= high) {
        return Err(format!(
            "window_scale must be between {} and {}",
            py_float(low),
            py_float(high)
        ));
    }
    let rows = match rows.unwrap_or(RowsSpec::Auto) {
        RowsSpec::Auto => auto_rows(columns, window_scale),
        RowsSpec::Count(n) => n,
    };
    let grid = Grid::new(columns, rows, window_scale);
    if grid.rows < 1 {
        return Err("rows must be at least 1, or \"auto\"".into());
    }
    if grid.items() > MAX_ITEMS {
        return Err(format!(
            "columns * rows must be {MAX_ITEMS} or less (slot names are SLOT01 to SLOT99)"
        ));
    }
    if grid.card_size().0 <= 0.0 {
        return Err("too many columns or rows: the cards would have no size".into());
    }
    Ok(grid)
}

/// Python `str(float)` for the simple values this module prints.
fn py_float(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Python default `format(value, "g")` (precision 6).
pub fn py_g(value: f64) -> String {
    const P: i32 = 6;
    if value == 0.0 {
        return "0".into();
    }
    let exp = value.abs().log10().floor() as i32;
    if (-4..P).contains(&exp) {
        let prec = (P - 1 - exp).max(0) as usize;
        strip_trailing_zeros(&format!("{value:.prec$}"))
    } else {
        let prec = (P - 1).max(0) as usize;
        let s = format!("{value:.prec$e}");
        python_sci(&s)
    }
}

fn strip_trailing_zeros(s: &str) -> String {
    let mut s = s.to_string();
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

fn python_sci(s: &str) -> String {
    let Some((mant, exp)) = s.split_once('e') else {
        return s.to_string();
    };
    let mant = strip_trailing_zeros(mant);
    let exp_n: i32 = exp.parse().unwrap_or(0);
    format!("{mant}e{exp_n:+03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_python() {
        let d = defaults();
        assert_eq!(d.get("columns").map(String::as_str), Some("8"));
        assert_eq!(d.get("rows").map(String::as_str), Some("auto"));
        assert_eq!(d.get("window_scale").map(String::as_str), Some("1.4"));
        assert_eq!(d.get("inventory_scale").map(String::as_str), Some("1.0"));
        assert_eq!(d.get("compact_tooltips").map(String::as_str), Some("true"));
        assert_eq!(d.get("write_logs").map(String::as_str), Some("true"));
    }

    #[test]
    fn auto_rows_matches_python() {
        assert_eq!(auto_rows(8, 1.4), 3);
        assert_eq!(auto_rows(10, 1.2), 4);
        assert_eq!(auto_rows(4, 1.0), 3);
        assert_eq!(auto_rows(6, 1.0), 3);
    }

    #[test]
    fn extra_size_and_card_size() {
        let g = Grid::new(8, 3, 1.4);
        assert_eq!(g.extra_size(), (216.0, 112.0));
        assert_eq!(g.card_size(), (74.5, 87.1));

        let v = Grid::new(4, 2, 1.0);
        assert_eq!(v.extra_size(), (0.0, 0.0));
        assert_eq!(v.card_size(), (102.5, 120.0));
        assert!(v.vanilla());

        let g60 = Grid::new(6, 3, 1.0);
        assert!(!g60.vanilla());
        assert_eq!(g60.extra_size(), (0.0, 0.0));
        assert_eq!(g60.card_size(), (62.6, 73.3));
    }

    #[test]
    fn vanilla_check_and_grid_from() {
        let g = grid_from(
            Some(4),
            Some(RowsSpec::Count(2)),
            Some(1.0),
        )
        .unwrap();
        assert!(g.vanilla());
        let auto = grid_from(Some(8), Some(RowsSpec::Auto), Some(1.4)).unwrap();
        assert_eq!(auto.rows, 3);
        assert_eq!(auto.items(), 24);
        assert!(grid_from(Some(0), None, None).is_err());
        assert!(grid_from(Some(10), Some(RowsSpec::Count(10)), Some(1.2)).is_err());
    }

    #[test]
    fn py_g_matches_python() {
        assert_eq!(py_g(1.4), "1.4");
        assert_eq!(py_g(1.0), "1");
        assert_eq!(py_g(1.2), "1.2");
        assert_eq!(py_g(216.0), "216");
        assert_eq!(py_g(112.0), "112");
        assert_eq!(py_g(74.5), "74.5");
        assert_eq!(py_g(87.1), "87.1");
    }
}
