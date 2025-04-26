use libtaurus::util::human_readable;

/// Test the human_readable() utility function
#[test]
fn test_human_readable() {
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
        TestCase(00000, "0 Hz"),
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
