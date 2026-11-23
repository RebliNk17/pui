//! Lightweight HTTP/1.1 request parser and response writer for Pui Remote.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
};

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 24 * 1024 * 1024;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query.get(key).map(String::as_str)
    }

    pub fn is_websocket_upgrade(&self) -> bool {
        self.header("upgrade")
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
            && self.header("sec-websocket-key").is_some()
    }

    pub fn bearer_token(&self) -> Option<&str> {
        if let Some(auth) = self.header("authorization") {
            if let Some(token) = auth
                .strip_prefix("Bearer ")
                .or_else(|| auth.strip_prefix("bearer "))
            {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        self.query_param("token").filter(|t| !t.is_empty())
    }
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn read_request(stream: &mut impl Read) -> io::Result<Request> {
    let mut buf = Vec::with_capacity(2048);
    let mut byte = [0u8; 1];
    let header_end;
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "EOF while reading HTTP headers",
            ));
        }
        buf.push(byte[0]);
        if buf.len() > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP headers too large",
            ));
        }
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            header_end = buf.len() - 4;
            break;
        }
    }

    let head = std::str::from_utf8(&buf[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8 in HTTP headers"))?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty HTTP request"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing HTTP method"))?
        .to_owned();
    let target = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing HTTP target"))?;

    let (raw_path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let path = percent_decode(raw_path);
    let query = parse_query(raw_query);

    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }

    let content_length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP request body exceeds limit",
        ));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body)?;
    }

    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

pub fn parse_query(raw: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(percent_decode(k), percent_decode(v));
    }
    out
}

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(val) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(val);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn write_json(
    stream: &mut impl Write,
    status: u16,
    reason: &str,
    value: &Value,
) -> io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    write_bytes(
        stream,
        status,
        reason,
        "application/json; charset=utf-8",
        &body,
    )
}

pub fn write_bytes(
    stream: &mut impl Write,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Headers: Authorization, Content-Type\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

pub fn write_options(stream: &mut impl Write) -> io::Result<()> {
    write_bytes(stream, 204, "No Content", "text/plain", &[])
}
