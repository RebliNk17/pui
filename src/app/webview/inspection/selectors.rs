//! Narrow CSS grammar: tags, ID/class identities and bounded combinators only.
//! Escapes are identifier data, never an avenue to attributes, pseudos or code.
const GENERATED_LIMIT: usize = 1024;

pub(super) fn selector(value: &str) -> Result<&str, &'static str> {
    let value = value.trim();
    if value.is_empty() || value.len() > GENERATED_LIMIT {
        return Err("Enter a bounded tag, #id or .class selector (at most 1024 bytes).");
    }
    if parse(value, false) {
        Ok(value)
    } else {
        Err(
            "Use at most six tags, #ids or .classes, with spaces or spaced >; no attributes, pseudos or scripts.",
        )
    }
}

pub(super) fn generated_selector_valid(value: &str) -> bool {
    !value.is_empty() && value.len() <= GENERATED_LIMIT && parse(value, true)
}

fn parse(value: &str, generated: bool) -> bool {
    let bytes = value.as_bytes();
    let mut pos = 0;
    let mut parts = 0;
    while pos < bytes.len() {
        parts += 1;
        if parts > 6 {
            return false;
        }
        let tag_start = pos;
        if bytes[pos].is_ascii_alphabetic() || bytes[pos] == b'_' {
            pos += 1;
            while pos < bytes.len()
                && (bytes[pos].is_ascii_alphanumeric() || matches!(bytes[pos], b'_' | b'-'))
            {
                pos += 1;
            }
        }
        let has_tag = pos > tag_start;
        if generated
            && (!has_tag
                || pos - tag_start > 64
                || !bytes[tag_start..pos]
                    .iter()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-'))
        {
            return false;
        }
        let mut identities = 0;
        let mut ids = 0;
        while pos < bytes.len() && matches!(bytes[pos], b'#' | b'.') {
            ids += usize::from(bytes[pos] == b'#');
            identities += 1;
            if ids > 1 || identities > 4 {
                return false;
            }
            pos += 1;
            let start = pos;
            let mut units = 0;
            while pos < bytes.len() {
                let b = bytes[pos];
                if b == b'\\' {
                    pos += 1;
                    if pos == bytes.len() {
                        return false;
                    }
                    let hex_start = pos;
                    while pos < bytes.len() && pos - hex_start < 6 && bytes[pos].is_ascii_hexdigit()
                    {
                        pos += 1;
                    }
                    if pos > hex_start {
                        let Ok(code) = u32::from_str_radix(&value[hex_start..pos], 16) else {
                            return false;
                        };
                        if code == 0 || code > 0x10ffff || (0xd800..=0xdfff).contains(&code) {
                            return false;
                        }
                        if bytes.get(pos) == Some(&b' ') {
                            pos += 1;
                        }
                    } else {
                        // CSS.escape emits backslash punctuation, not escaped newlines.
                        if !bytes[pos].is_ascii_punctuation() {
                            return false;
                        }
                        pos += 1;
                    }
                } else if b.is_ascii_alphabetic()
                    || b == b'_'
                    || (units > 0 && (b.is_ascii_digit() || b == b'-'))
                    || (units == 0
                        && b == b'-'
                        && bytes.get(pos + 1).is_some_and(|next| {
                            next.is_ascii_alphabetic()
                                || matches!(*next, b'_' | b'-' | b'\\')
                                || *next >= 0x80
                        }))
                {
                    pos += 1;
                } else if b >= 0x80 {
                    let ch = value[pos..].chars().next().unwrap();
                    if ch.is_control() || ch.is_whitespace() {
                        return false;
                    }
                    pos += ch.len_utf8();
                } else {
                    break;
                }
                units += 1;
                if units > 128 {
                    return false;
                }
            }
            if pos == start {
                return false;
            }
        }
        if !has_tag && identities == 0 {
            return false;
        }
        if pos == bytes.len() {
            return true;
        }
        if bytes[pos] != b' ' {
            return false;
        }
        while bytes.get(pos) == Some(&b' ') {
            pos += 1;
        }
        if bytes.get(pos) == Some(&b'>') {
            pos += 1;
            if bytes.get(pos) != Some(&b' ') {
                return false;
            }
            while bytes.get(pos) == Some(&b' ') {
                pos += 1;
            }
        } else if generated {
            return false;
        }
        if pos == bytes.len() {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaped_identity_is_bounded_data_not_css_syntax() {
        for query in [
            "h1#café",
            "div#\\31 23",
            "span.a\\:b",
            "main > div.שלום",
            "div#x\\[y\\]",
            "div.-foo",
            "div.--foo",
        ] {
            assert!(generated_selector_valid(query), "{query}");
            assert!(selector(query).is_ok());
        }
        for query in [
            "div[x]",
            "div:hover",
            "div,span",
            "div\\61",
            "div#\\0 ",
            "div#\\110000 ",
            "div#\\d800 ",
            "div#\\\n",
            "div#x()",
            "div #x",
            "div > > p",
            "div#x#y",
            "div.a.b.c.d.e",
            "div > a > b > c > d > e > f",
        ] {
            assert!(!generated_selector_valid(query), "{query}");
        }
        assert!(!generated_selector_valid(&format!(
            "div#{}",
            "a".repeat(129)
        )));
        assert!(!generated_selector_valid(&"x".repeat(1025)));
    }
}
