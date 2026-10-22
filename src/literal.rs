//! Decoding of literal text: string and template escapes, numbers, escaped identifiers.
//!
//! JavaScript strings are UTF-16 and may hold lone surrogates, which a Rust `String` cannot.
//! A lone surrogate escape is decoded as U+FFFD here. That is lossy for the rare program
//! that builds strings from unpaired surrogates; fixing it needs a WTF-8 string type.

/// Decodes the body of a string literal (the text between the quotes).
/// With `strict`, legacy octal escapes such as `\07` and `\8` are errors.
pub fn cook_string(raw: &str, strict: bool) -> Result<String, &'static str> {
    cook(raw, if strict { Mode::StrictString } else { Mode::SloppyString })
}

/// Decodes the raw text of a template chunk. `None` means the chunk has an invalid escape,
/// which is legal only in a tagged template.
pub fn cook_template(raw: &str) -> Option<String> {
    cook(raw, Mode::Template).ok()
}

/// Template raw values normalize CRLF and lone CR to LF.
pub fn template_raw(raw: &str) -> String {
    if !raw.contains('\r') {
        return raw.to_string();
    }
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    SloppyString,
    StrictString,
    Template,
}

fn cook(raw: &str, mode: Mode) -> Result<String, &'static str> {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            let end = raw[i..].find('\\').map_or(bytes.len(), |p| i + p);
            out.push_str(&raw[i..end]);
            i = end;
            continue;
        }
        i += 1;
        let Some(&c) = bytes.get(i) else { return Err("unterminated escape") };
        i += 1;
        match c {
            b'n' => out.push('\n'),
            b't' => out.push('\t'),
            b'r' => out.push('\r'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{C}'),
            b'v' => out.push('\u{B}'),
            b'\r' => {
                if bytes.get(i) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => {}
            b'0'..=b'7' => {
                let lone_zero = c == b'0' && !bytes.get(i).is_some_and(|d| d.is_ascii_digit());
                if lone_zero {
                    out.push('\0');
                } else if mode == Mode::Template {
                    return Err("octal escapes are not allowed in templates");
                } else if mode == Mode::StrictString {
                    return Err("octal escapes are not allowed in strict mode");
                } else {
                    // Up to three digits, and the value must stay below 256.
                    let max_len = if c <= b'3' { 3 } else { 2 };
                    let mut value = (c - b'0') as u32;
                    let mut len = 1;
                    while len < max_len {
                        match bytes.get(i) {
                            Some(&d @ b'0'..=b'7') => {
                                value = value * 8 + (d - b'0') as u32;
                                i += 1;
                                len += 1;
                            }
                            _ => break,
                        }
                    }
                    out.push(char::from_u32(value).unwrap());
                }
            }
            b'8' | b'9' => match mode {
                Mode::SloppyString => out.push(c as char),
                Mode::StrictString => return Err("\\8 and \\9 are not allowed in strict mode"),
                Mode::Template => return Err("\\8 and \\9 are not allowed in templates"),
            },
            b'x' => {
                let value = hex_digits(bytes, &mut i, 2).ok_or("malformed hex escape")?;
                out.push(char::from_u32(value).unwrap());
            }
            b'u' => {
                let unit = unicode_escape(bytes, &mut i)?;
                if (0xD800..0xDC00).contains(&unit) {
                    // A high surrogate may pair with an immediately following low one.
                    if bytes.get(i) == Some(&b'\\') && bytes.get(i + 1) == Some(&b'u') {
                        let mut j = i + 2;
                        if let Ok(low) = unicode_escape(bytes, &mut j) {
                            if (0xDC00..0xE000).contains(&low) {
                                let c = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                out.push(char::from_u32(c).unwrap());
                                i = j;
                                continue;
                            }
                        }
                    }
                }
                out.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
            }
            _ => {
                // Any other escaped character stands for itself. This includes the line
                // separators U+2028/U+2029, which continue a line like \n does.
                i -= 1;
                let ch = raw[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch != '\u{2028}' && ch != '\u{2029}' {
                    out.push(ch);
                }
            }
        }
    }
    Ok(out)
}

fn hex_digits(bytes: &[u8], i: &mut usize, count: usize) -> Option<u32> {
    let mut value = 0;
    for k in 0..count {
        value = value * 16 + (*bytes.get(*i + k)? as char).to_digit(16)?;
    }
    *i += count;
    Some(value)
}

/// Cursor is just past the `u`. Returns a code point or a UTF-16 code unit.
fn unicode_escape(bytes: &[u8], i: &mut usize) -> Result<u32, &'static str> {
    if bytes.get(*i) == Some(&b'{') {
        let mut j = *i + 1;
        let mut value: u32 = 0;
        let mut digits = 0;
        while let Some(d) = bytes.get(j).and_then(|&b| (b as char).to_digit(16)) {
            value = value.checked_mul(16).and_then(|v| v.checked_add(d)).ok_or("unicode escape is out of range")?;
            digits += 1;
            j += 1;
        }
        if digits == 0 || bytes.get(j) != Some(&b'}') {
            return Err("malformed unicode escape");
        }
        if value > 0x10FFFF {
            return Err("unicode escape is out of range");
        }
        *i = j + 1;
        Ok(value)
    } else {
        hex_digits(bytes, i, 4).ok_or("malformed unicode escape")
    }
}

/// Decodes `\u` escapes in an identifier's source text.
pub fn decode_identifier(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'u') {
            let mut j = i + 2;
            // The lexer already validated this escape.
            if let Ok(value) = unicode_escape(bytes, &mut j) {
                out.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
                i = j;
                continue;
            }
        }
        let ch = raw[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Value of a numeric literal's source text. Radix literals accumulate in `f64`, so integers
/// beyond 2^53 may differ from the exact value in the last place.
pub fn parse_number(text: &str) -> f64 {
    let bytes = text.as_bytes();
    if bytes.len() > 1 && bytes[0] == b'0' {
        match bytes[1] {
            b'x' | b'X' => return radix_value(&text[2..], 16),
            b'o' | b'O' => return radix_value(&text[2..], 8),
            b'b' | b'B' => return radix_value(&text[2..], 2),
            b'0'..=b'7' if is_legacy_octal(text) => return radix_value(&text[1..], 8),
            _ => {}
        }
    }
    let cleaned;
    let digits = if text.contains('_') {
        cleaned = text.replace('_', "");
        cleaned.as_str()
    } else {
        text
    };
    digits.parse().unwrap_or(f64::NAN)
}

/// `017`: a leading zero followed only by octal digits. Invalid in strict mode.
pub fn is_legacy_octal(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 1 && bytes[0] == b'0' && bytes[1..].iter().all(|b| (b'0'..=b'7').contains(b))
}

/// `089`: a leading zero in a decimal. Also invalid in strict mode.
pub fn is_leading_zero_decimal(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 1 && bytes[0] == b'0' && bytes[1].is_ascii_digit() && !is_legacy_octal(text)
}

fn radix_value(digits: &str, radix: u32) -> f64 {
    let mut value = 0.0;
    for c in digits.chars() {
        if let Some(d) = c.to_digit(radix) {
            value = value * radix as f64 + d as f64;
        }
    }
    value
}

/// Normalizes a bigint literal's text: drops `_` and the `n` suffix.
pub fn bigint_digits(text: &str) -> String {
    text.trim_end_matches('n').replace('_', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Backslashes are spelled `~` in test inputs; see the lexer tests.
    fn esc(src: &str) -> String {
        src.replace('~', "\\")
    }

    fn cook_ok(src: &str) -> String {
        cook_string(&esc(src), false).unwrap()
    }

    #[test]
    fn simple_escapes() {
        assert_eq!(cook_ok("a~nb~tc~\\d~'e~\"f"), "a\nb\tc\\d'e\"f");
        assert_eq!(cook_ok("~b~f~v~r~0"), "\u{8}\u{C}\u{B}\r\0");
        assert_eq!(cook_ok("plain é 日本"), "plain é 日本");
        assert_eq!(cook_ok("~q~é"), "qé", "unknown escapes stand for themselves");
    }

    #[test]
    fn line_continuations() {
        assert_eq!(cook_ok("a~\nb"), "ab");
        assert_eq!(cook_ok("a~\r\nb"), "ab");
        assert_eq!(cook_ok("a~\u{2028}b"), "ab");
    }

    #[test]
    fn hex_and_unicode() {
        assert_eq!(cook_ok("~x41~u0042~u{43}~u{1F600}"), "ABC\u{1F600}");
        assert_eq!(cook_ok("~uD83D~uDE00"), "\u{1F600}", "surrogate pair");
        assert_eq!(cook_ok("~uD83D"), "\u{FFFD}", "lone surrogates are lossy");
        assert!(cook_string(&esc("~x4"), false).is_err());
        assert!(cook_string(&esc("~u12"), false).is_err());
        assert!(cook_string(&esc("~u{110000}"), false).is_err());
        assert!(cook_string(&esc("~u{}"), false).is_err());
    }

    #[test]
    fn legacy_octal_by_mode() {
        assert_eq!(cook_ok("~101~7~377~400"), "A\u{7}\u{FF}\u{20}0");
        assert_eq!(cook_ok("~8~9"), "89");
        assert!(cook_string(&esc("~101"), true).is_err());
        assert!(cook_string(&esc("~8"), true).is_err());
        assert_eq!(cook_string(&esc("~0"), true).unwrap(), "\0", "a lone \\0 is always fine");
        assert!(cook_string(&esc("~01"), true).is_err());
    }

    #[test]
    fn templates() {
        assert_eq!(cook_template(&esc("a~nb")).as_deref(), Some("a\nb"));
        assert_eq!(cook_template(&esc("~xZ")), None);
        assert_eq!(cook_template(&esc("~01")), None);
        assert_eq!(cook_template(&esc("~0")).as_deref(), Some("\0"));
        assert_eq!(template_raw("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_number("0"), 0.0);
        assert_eq!(parse_number("123"), 123.0);
        assert_eq!(parse_number("1_000"), 1000.0);
        assert_eq!(parse_number("1."), 1.0);
        assert_eq!(parse_number(".5"), 0.5);
        assert_eq!(parse_number("1e3"), 1000.0);
        assert_eq!(parse_number("1E-2"), 0.01);
        assert_eq!(parse_number("0xFF"), 255.0);
        assert_eq!(parse_number("0XfF"), 255.0);
        assert_eq!(parse_number("0o17"), 15.0);
        assert_eq!(parse_number("0b101"), 5.0);
        assert_eq!(parse_number("0xA_B"), 171.0);
        assert_eq!(parse_number("017"), 15.0, "legacy octal");
        assert_eq!(parse_number("089"), 89.0, "leading-zero decimal");
        assert_eq!(parse_number("0.5"), 0.5);
        assert_eq!(parse_number("1e400"), f64::INFINITY);
    }

    #[test]
    fn legacy_number_forms() {
        assert!(is_legacy_octal("017"));
        assert!(!is_legacy_octal("0"));
        assert!(!is_legacy_octal("089"));
        assert!(is_leading_zero_decimal("089"));
        assert!(!is_leading_zero_decimal("0.5"));
        assert!(!is_leading_zero_decimal("017"));
    }

    #[test]
    fn identifiers_and_bigints() {
        assert_eq!(decode_identifier(&esc("a~u0062c")), "abc");
        assert_eq!(decode_identifier(&esc("~u{1d7d8}x")), "\u{1d7d8}x");
        assert_eq!(decode_identifier("plain"), "plain");
        assert_eq!(bigint_digits("1_000n"), "1000");
        assert_eq!(bigint_digits("0xFFn"), "0xFF");
    }
}
