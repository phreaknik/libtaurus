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

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Test the human_readable() utility function
    #[test]
    fn human_readable_util() {
        struct TestCase<'a>(i64, &'a str);
        let cases = [
            TestCase(-10000000000000001, "-10 PHz"),
            TestCase(-10000000000000000, "-10 PHz"),
            TestCase(-9999999999999999, "-9999 THz"),
            TestCase(-10000000000001, "-10 THz"),
            TestCase(-10000000000000, "-10 THz"),
            TestCase(-9999999999999, "-9999 GHz"),
            TestCase(-10000000001, "-10 GHz"),
            TestCase(-10000000000, "-10 GHz"),
            TestCase(-9999999999, "-9999 MHz"),
            TestCase(-10000001, "-10 MHz"),
            TestCase(-10000000, "-10 MHz"),
            TestCase(-9999999, "-9999 kHz"),
            TestCase(-10001, "-10 kHz"),
            TestCase(-10000, "-10 kHz"),
            TestCase(-9999, "-9999 Hz"),
            TestCase(-10, "-10 Hz"),
            TestCase(-1, "-1 Hz"),
            TestCase(0, "0 Hz"),
            TestCase(1, "1 Hz"),
            TestCase(10, "10 Hz"),
            TestCase(9999, "9999 Hz"),
            TestCase(10000, "10 kHz"),
            TestCase(10001, "10 kHz"),
            TestCase(9999999, "9999 kHz"),
            TestCase(10000000, "10 MHz"),
            TestCase(10000001, "10 MHz"),
            TestCase(9999999999, "9999 MHz"),
            TestCase(10000000000, "10 GHz"),
            TestCase(10000000001, "10 GHz"),
            TestCase(9999999999999, "9999 GHz"),
            TestCase(10000000000000, "10 THz"),
            TestCase(10000000000001, "10 THz"),
            TestCase(9999999999999999, "9999 THz"),
            TestCase(10000000000000000, "10 PHz"),
            TestCase(10000000000000001, "10 PHz"),
        ];
        for case in cases {
            assert_eq!(human_readable(case.0, "Hz"), case.1)
        }
    }
}
