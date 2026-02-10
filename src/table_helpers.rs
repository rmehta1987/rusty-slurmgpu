use comfy_table::{
    presets::UTF8_FULL_CONDENSED, Attribute, Cell, CellAlignment, Color, ContentArrangement, Table,
};

use crate::constants::{
    EXCELLENT_EFFICIENCY_THRESHOLD, GOOD_EFFICIENCY_THRESHOLD, NO_DATA, POOR_EFFICIENCY_THRESHOLD,
};

/// Create a new table with the standard preset and arrangement.
pub(crate) fn new_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

/// Create a bold white left-aligned header cell.
pub(crate) fn hdr(name: &str) -> Cell {
    Cell::new(name)
        .add_attribute(Attribute::Bold)
        .fg(Color::White)
}

/// Create a bold white right-aligned header cell.
pub(crate) fn hdr_right(name: &str) -> Cell {
    hdr(name).set_alignment(CellAlignment::Right)
}

/// Get color for efficiency percentage.
pub(crate) fn efficiency_color(value: &str) -> Color {
    if value == NO_DATA || value.is_empty() {
        return Color::White;
    }

    match value.trim_end_matches('%').parse::<f64>() {
        Ok(v) if v >= EXCELLENT_EFFICIENCY_THRESHOLD => Color::Green,
        Ok(v) if v >= GOOD_EFFICIENCY_THRESHOLD => Color::Yellow,
        Ok(_) => Color::Red,
        Err(_) => Color::White,
    }
}

/// Check if efficiency value is critically low (< 30%) for bold formatting.
pub(crate) fn is_critical_efficiency(value: &str) -> bool {
    if value == NO_DATA || value.is_empty() {
        return false;
    }
    value
        .trim_end_matches('%')
        .parse::<f64>()
        .map_or(false, |v| v < POOR_EFFICIENCY_THRESHOLD)
}

/// Create a cell with efficiency color, adding bold for critical values.
pub(crate) fn efficiency_cell(value: &str) -> Cell {
    let cell = Cell::new(value)
        .fg(efficiency_color(value))
        .set_alignment(CellAlignment::Right);
    if is_critical_efficiency(value) {
        cell.add_attribute(Attribute::Bold)
    } else {
        cell
    }
}

/// Get color for job state.
pub(crate) fn state_color(state: &str) -> Color {
    match state.to_uppercase().as_str() {
        "COMPLETED" => Color::Green,
        "FAILED" => Color::Red,
        "CANCELLED" | "TIMEOUT" => Color::Yellow,
        "PENDING" => Color::Blue,
        "RUNNING" => Color::Cyan,
        _ => Color::White,
    }
}

/// Get color for resource utilization percentage.
/// High utilization = Green (healthy), Low = Red (underused or needs attention).
pub(crate) fn utilization_color(util_percent: f64) -> Color {
    if util_percent >= EXCELLENT_EFFICIENCY_THRESHOLD {
        Color::Green
    } else if util_percent >= GOOD_EFFICIENCY_THRESHOLD {
        Color::Yellow
    } else {
        Color::Red
    }
}
