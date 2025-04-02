use std::{fmt::Display, ops::DivAssign};

use num::Num;

/// Formats a number into a human readable string
pub fn human_readable<T>(mut num: T, units: &str) -> String
where
    T: Num + PartialOrd + PartialEq + DivAssign + Display + From<u32>,
{
    let prefixes = ["", "k", "M", "G", "T", "P", "E", "Z", "Y", "R", "Q"];
    let mut pre = 0;
    loop {
        let one_thousand = T::from(1_000);
        let ten_thousand = T::from(10_000);
        if num > ten_thousand {
            num /= one_thousand;
            pre += 1;
        } else {
            return format!("{num} {}{}", prefixes[pre], units).to_string();
        }
    }
}
