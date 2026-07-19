//! Dump the built-in function registry as JSON, one object per registered
//! function: name, namespace (the stored `_xlfn.`/`_xlfn._xlws.` prefix), min
//! args, variadic, volatile. Consumed as a drift-check baseline for an external
//! Excel-function catalog (formulon).
//!
//!     cargo run --release -p formualizer-eval --example dump_registry
use formualizer_eval::builtins::load_builtins;
use formualizer_eval::function_registry::snapshot_registered;

fn main() {
    load_builtins();
    // (name, namespace, min_args, variadic, volatile), sorted for a stable diff.
    let mut rows: Vec<(String, String, usize, bool, bool)> = snapshot_registered()
        .into_iter()
        .map(|(ns, name, f)| (name, ns, f.min_args(), f.variadic(), f.volatile()))
        .collect();
    rows.sort();

    println!("[");
    for (i, (name, ns, min_args, variadic, volatile)) in rows.iter().enumerate() {
        let comma = if i + 1 < rows.len() { "," } else { "" };
        println!(
            "  {{\"name\":\"{name}\",\"namespace\":\"{ns}\",\"min_args\":{min_args},\"variadic\":{variadic},\"volatile\":{volatile}}}{comma}"
        );
    }
    println!("]");
}
