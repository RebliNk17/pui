//! One applied column, with explicit empty fields for absent ragged cells.
use super::*;
pub(super) fn serialize(
    source: &str,
    index: &Index,
    rows: projection::Rows<'_>,
    column: usize,
    delimiter: u8,
) -> Result<String, &'static str> {
    if !matches!(delimiter, b',' | b'\t') || column >= index.columns {
        return Err("Active column is unavailable; nothing copied.");
    }
    let value = |position| {
        rows.record(position)
            .filter(|&record| record < index.records.len())
            .map(|record| index.value(source, record, column))
    };
    let mut bytes = 0usize;
    for position in 0..rows.len() {
        let value = value(position).ok_or("Table view is unavailable; nothing copied.")?;
        bytes = bytes
            .checked_add(
                encoded_len(value, delimiter)
                    .and_then(|len| len.checked_add(1))
                    .ok_or("Copy exceeds 512 KiB; nothing copied.")?,
            )
            .ok_or("Copy exceeds 512 KiB; nothing copied.")?;
        if bytes > MAX_OUTPUT {
            return Err("Copy exceeds 512 KiB; nothing copied.");
        }
    }
    let mut output = String::with_capacity(bytes);
    for position in 0..rows.len() {
        let value = value(position).unwrap();
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
        output.push('\n');
    }
    debug_assert_eq!(bytes, output.len());
    Ok(output)
}
