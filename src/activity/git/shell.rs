//! Small conservative shell reader: only literal commands, quotes and separators.
//! Anything requiring shell evaluation stays a generic command in the UI.
#[derive(Debug)]
pub(super) struct Segment {
    pub words: Vec<String>,
    pub and_next: bool,
}

pub(super) fn segments(command: &str) -> Option<Vec<Segment>> {
    let mut chars = command.chars().peekable();
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), c) => word.push(c),
            (_, '$' | '`') => return None,
            (_, '\\') => {
                let escaped = chars.next()?;
                if escaped != '\n' {
                    word.push(escaped);
                    started = true;
                }
            }
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '|' | '(' | ')' | '{' | '}' | '<' | '>' | '#' | '*' | '?' | '~' | '[' | ']') => {
                return None;
            }
            (None, '&' | ';' | '\n') => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
                let and_next = c == '&';
                if and_next && chars.next() != Some('&') {
                    return None;
                }
                if words.is_empty() {
                    if c == '\n' {
                        continue;
                    }
                    return None;
                }
                segments.push(Segment {
                    words: std::mem::take(&mut words),
                    and_next,
                });
                if segments.len() > 128 {
                    return None;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
        if words.len() > 512 {
            return None;
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(word);
    }
    if !words.is_empty() {
        segments.push(Segment {
            words,
            and_next: false,
        });
    } else if segments.last().is_some_and(|segment| segment.and_next) {
        return None;
    }
    (!segments.is_empty()).then_some(segments)
}
