use std::cmp;

/// Formats a number into a human readable string
pub fn human_readable(num: i64, units: &str) -> String {
    let sign = if num < 0 { "-" } else { "" };
    let mut num = cmp::max(num, -num);
    let prefixes = ["", "k", "M", "G", "T", "P"];
    let mut pre = 0;
    loop {
        if num >= 10_000 {
            num /= 1_000;
            pre += 1;
        } else {
            return format!("{sign}{num} {}{units}", prefixes[pre]).to_string();
        }
    }
}

pub trait Randomizer {
    /// Generate a randomized object, useful for testing.
    fn random() -> Self;
}
