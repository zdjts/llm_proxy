//! Hand-rolled CSV field escaper — ADR-012 §5.
//!
//! Wraps a value in double-quotes and doubles internal quotes if the value
//! contains `,`, `"`, `\n`, or `\r`.  Otherwise returns the value as-is.

pub fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}
