use num::Num;
use std::{cmp, fmt::Display, ops::DivAssign};

/// Formats a number into a human readable string
pub fn human_readable<T>(num: T, units: &str) -> String
where
    T: Num + PartialOrd + Ord + Copy + PartialEq + DivAssign + Display + From<u32>,
{
    let sign = if num < T::zero() { "-" } else { "" };
    let mut num = cmp::max(num, T::zero() - num);
    let prefixes = ["", "k", "M", "G", "T", "P"];
    let mut pre = 0;
    loop {
        let one_thousand = T::from(1_000);
        let ten_thousand = T::from(10_000);
        if num >= ten_thousand {
            num /= one_thousand;
            pre += 1;
        } else {
            return format!("{sign}{num} {}{units}", prefixes[pre]).to_string();
        }
    }
}
