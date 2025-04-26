use libtaurus::hash::Hash;

#[test]
fn corner_cases() {
    pub struct HashTestCase<'a> {
        pub decoded: Hash,
        pub long_hex: &'a str,
        pub short_hex: &'a str,
    }
    for test in [
        HashTestCase {
            decoded: Hash::with_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ]),
            long_hex: "0000000000000000000000000000000000000000000000000000000000000000",
            short_hex: "00000000..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
            ]),
            long_hex: "8000000000000000000000000000000000000000000000000000000000000000",
            short_hex: "80000000..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 1,
            ]),
            long_hex: "0000000000000000000000000000000000000000000000000000000000000001",
            short_hex: "00000000..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            long_hex: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            short_hex: "ffffffff..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            long_hex: "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            short_hex: "7fffffff..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254,
            ]),
            long_hex: "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
            short_hex: "ffffffff..",
        },
    ] {
        assert_eq!(test.decoded.to_hex(), test.long_hex);
        assert_eq!(test.decoded.to_short_hex(), test.short_hex);
    }
}
