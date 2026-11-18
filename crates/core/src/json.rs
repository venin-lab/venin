//! The record's JSON writer: printable ASCII, integers only, and every character from U+007F up as a lowercase
//! `\uxxxx` escape (UTF-16 surrogate pairs above U+FFFF). The bytes it writes are the bytes hashed.

use serde::Serialize;
use serde_json::ser::{Formatter, Serializer};
use std::io::{self, Write};

/// The largest integer every JSON reader holds exactly (2^53 - 1).
pub const MAX_SAFE: u64 = (1 << 53) - 1;

pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut out = Vec::new();
    value.serialize(&mut Serializer::with_formatter(&mut out, Ascii))?;
    Ok(out)
}

pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    to_vec(value).map(|bytes| String::from_utf8(bytes).expect("the writer emits ASCII only"))
}

struct Ascii;

fn unsafe_integer(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!(
        "integer {value} is beyond 2^53-1; store it as a decimal string"
    ))
}

fn checked<W: ?Sized + Write>(
    w: &mut W,
    magnitude: u128,
    value: impl std::fmt::Display,
) -> io::Result<()> {
    if magnitude > u128::from(MAX_SAFE) {
        return Err(unsafe_integer(value));
    }
    write!(w, "{value}")
}

impl Formatter for Ascii {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        w: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (i, c) in fragment.char_indices() {
            if u32::from(c) < 0x7f {
                continue;
            }
            w.write_all(&fragment.as_bytes()[start..i])?;
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                write!(w, "\\u{unit:04x}")?;
            }
            start = i + c.len_utf8();
        }
        w.write_all(&fragment.as_bytes()[start..])
    }

    fn write_i64<W: ?Sized + Write>(&mut self, w: &mut W, value: i64) -> io::Result<()> {
        checked(w, u128::from(value.unsigned_abs()), value)
    }

    fn write_u64<W: ?Sized + Write>(&mut self, w: &mut W, value: u64) -> io::Result<()> {
        checked(w, u128::from(value), value)
    }

    fn write_i128<W: ?Sized + Write>(&mut self, w: &mut W, value: i128) -> io::Result<()> {
        checked(w, value.unsigned_abs(), value)
    }

    fn write_u128<W: ?Sized + Write>(&mut self, w: &mut W, value: u128) -> io::Result<()> {
        checked(w, value, value)
    }

    fn write_f32<W: ?Sized + Write>(&mut self, _: &mut W, value: f32) -> io::Result<()> {
        Err(io::Error::other(format!(
            "float {value} in the record; use an integer"
        )))
    }

    fn write_f64<W: ?Sized + Write>(&mut self, _: &mut W, value: f64) -> io::Result<()> {
        Err(io::Error::other(format!(
            "float {value} in the record; use an integer"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(code_points: &[u32]) -> String {
        code_points
            .iter()
            .map(|&c| char::from_u32(c).expect("valid code point"))
            .collect()
    }

    #[test]
    fn golden_astral_record_is_byte_exact() {
        #[derive(Serialize)]
        struct Golden {
            name: String,
            n: u64,
            neg: i64,
            ok: bool,
            none: Option<u8>,
            list: Vec<&'static str>,
        }
        let mut name = String::from("caf");
        name.push_str(&text(&[0xe9, 0x20, 0x1f600, 0x7f, 0x1, 0x9, 0xa, 0x200b]));
        name.push_str("\"\\/");
        let golden = Golden {
            name,
            n: MAX_SAFE,
            neg: -(MAX_SAFE as i64),
            ok: true,
            none: None,
            list: vec!["a", "b"],
        };
        let expected = concat!(
            r#"{"name":"caf\u00e9 \ud83d\ude00\u007f\u0001\t\n\u200b\"\\/","#,
            r#""n":9007199254740991,"neg":-9007199254740991,"ok":true,"none":null,"list":["a","b"]}"#
        );
        assert_eq!(to_vec(&golden).expect("writes"), expected.as_bytes());
    }

    #[test]
    fn output_is_printable_ascii_and_parses_back() {
        let original = text(&[0x41, 0x10ffff, 0xfeff, 0xe0041, 0x7e]);
        let written = to_string(&original).expect("writes");
        assert!(written.bytes().all(|b| (0x20..0x7f).contains(&b)));
        let back: String = serde_json::from_str(&written).expect("parses");
        assert_eq!(back, original);
    }

    #[test]
    fn maps_are_sorted_and_writes_are_deterministic() {
        let value = json!({"b": 1, "a": {"d": 2, "c": 3}});
        assert_eq!(
            to_string(&value).expect("writes"),
            r#"{"a":{"c":3,"d":2},"b":1}"#
        );
        assert_eq!(
            to_vec(&value).expect("writes"),
            to_vec(&value).expect("writes")
        );
    }

    #[test]
    fn floats_and_unsafe_integers_are_refused() {
        assert!(to_vec(&1.5f64).is_err());
        assert!(to_vec(&json!({"x": 0.25})).is_err());
        assert!(to_vec(&(MAX_SAFE + 1)).is_err());
        assert!(to_vec(&(-(MAX_SAFE as i64) - 1)).is_err());
        assert!(to_vec(&u128::from(u64::MAX)).is_err());
        assert_eq!(
            to_string(&"12345678901234567890").expect("writes"),
            r#""12345678901234567890""#
        );
    }
}
