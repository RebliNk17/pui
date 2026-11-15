//! Applied-view rectangle, explicit CSV/TSV with ragged empty fields.
use super::*;
pub(in crate::app::files::delimited) fn serialize(
    source: &str,
    index: &Index,
    rows: projection::Rows<'_>,
    first: (usize, usize),
    last: (usize, usize),
) -> Result<String, &'static str> {
    serialize_as(source, index, rows, first, last, b'\t')
}
pub(super) fn serialize_as(
    source: &str,
    index: &Index,
    rows: projection::Rows<'_>,
    first: (usize, usize),
    last: (usize, usize),
    delimiter: u8,
) -> Result<String, &'static str> {
    serialize_limited(source, index, rows, first, last, delimiter, MAX_OUTPUT)
}
pub(super) fn serialize_limited(
    source: &str,
    index: &Index,
    rows: projection::Rows<'_>,
    first: (usize, usize),
    last: (usize, usize),
    delimiter: u8,
    limit: usize,
) -> Result<String, &'static str> {
    if !matches!(delimiter, b',' | b'\t') {
        return Err("Unsupported selection delimiter; nothing copied.");
    }
    if first.0 > last.0 || first.1 > last.1 || last.0 >= rows.len() || last.1 >= index.columns {
        return Err("Selection unavailable; nothing copied.");
    }
    let mut bytes = 0usize;
    for position in first.0..=last.0 {
        let record = rows
            .record(position)
            .filter(|&record| record < index.records.len())
            .ok_or("Selection unavailable; nothing copied.")?;
        for column in first.1..=last.1 {
            bytes = bytes
                .checked_add(
                    encoded_len(index.value(source, record, column), delimiter)
                        .and_then(|len| len.checked_add(1))
                        .ok_or("Selection exceeds 512 KiB; nothing copied.")?,
                )
                .ok_or("Selection exceeds 512 KiB; nothing copied.")?;
            if bytes > limit {
                return Err("Selection exceeds 512 KiB; nothing copied.");
            }
        }
    }
    let mut output = String::with_capacity(bytes);
    for position in first.0..=last.0 {
        let record = rows.record(position).unwrap();
        for column in first.1..=last.1 {
            if column > first.1 {
                output.push(char::from(delimiter));
            }
            let value = index.value(source, record, column);
            let quoted = value.is_empty()
                || value
                    .bytes()
                    .any(|byte| matches!(byte, b'"' | b'\r' | b'\n') || byte == delimiter);
            if quoted {
                output.push('"');
                for ch in value.chars() {
                    if ch == '"' {
                        output.push('"');
                    }
                    output.push(ch);
                }
                output.push('"');
            } else {
                output.push_str(value);
            }
        }
        output.push('\n');
    }
    debug_assert_eq!(output.len(), bytes);
    Ok(output)
}
