//! Pure-Rust RFC 6455 WebSocket server framing and RFC 3174 SHA-1 handshake.
//! Zero extra Cargo dependencies so offline/locked builds remain untouched.
use base64::Engine as _;
use std::{
    io::{self, Read, Write},
    net::TcpStream,
};

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_WS_PAYLOAD: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsMessage {
    Text(String),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// Pure-Rust SHA-1 (RFC 3174) used solely for RFC 6455 Sec-WebSocket-Accept.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, h) in [h0, h1, h2, h3, h4].into_iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&h.to_be_bytes());
    }
    out
}

pub fn accept_key(sec_websocket_key: &str) -> String {
    let mut input = Vec::with_capacity(sec_websocket_key.len() + WS_GUID.len());
    input.extend_from_slice(sec_websocket_key.trim().as_bytes());
    input.extend_from_slice(WS_GUID.as_bytes());
    let digest = sha1(&input);
    base64::engine::general_purpose::STANDARD.encode(digest)
}

pub fn write_handshake(stream: &mut TcpStream, sec_websocket_key: &str) -> io::Result<()> {
    let accept = accept_key(sec_websocket_key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

pub fn encode_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 10);
    frame.push(0x80 | (opcode & 0x0F));
    let len = payload.len();
    if len <= 125 {
        frame.push(len as u8);
    } else if len <= 65_535 {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    frame
}

pub fn write_text(stream: &mut impl Write, text: &str) -> io::Result<()> {
    stream.write_all(&encode_frame(0x1, text.as_bytes()))?;
    stream.flush()
}

pub fn write_pong(stream: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let bounded = &payload[..payload.len().min(125)];
    stream.write_all(&encode_frame(0xA, bounded))?;
    stream.flush()
}

pub fn write_close(stream: &mut impl Write) -> io::Result<()> {
    stream.write_all(&encode_frame(0x8, &[]))?;
    stream.flush()
}

/// Encodes a client-to-server masked RFC 6455 frame.
pub fn encode_client_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | (opcode & 0x0F));
    let len = payload.len();
    if len <= 125 {
        frame.push(0x80 | (len as u8));
    } else if len <= 65_535 {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0x55AA_33CC);
    let mask = [
        (nanos & 0xFF) as u8 ^ 0xA5,
        ((nanos >> 8) & 0xFF) as u8 ^ 0x5A,
        ((nanos >> 16) & 0xFF) as u8 ^ 0x3C,
        ((nanos >> 24) & 0xFF) as u8 ^ 0xC3,
    ];
    frame.extend_from_slice(&mask);
    for (i, b) in payload.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    frame
}

pub fn write_client_text(stream: &mut impl Write, text: &str) -> io::Result<()> {
    stream.write_all(&encode_client_frame(0x1, text.as_bytes()))?;
    stream.flush()
}

pub fn write_client_pong(stream: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let bounded = &payload[..payload.len().min(125)];
    stream.write_all(&encode_client_frame(0xA, bounded))?;
    stream.flush()
}

pub fn write_client_close(stream: &mut impl Write) -> io::Result<()> {
    stream.write_all(&encode_client_frame(0x8, &[]))?;
    stream.flush()
}

/// Performs an RFC 6455 client HTTP Upgrade handshake over an existing TCP stream.
pub fn connect_client(
    host: &str,
    port: u16,
    path_and_query: &str,
    bearer_token: &str,
) -> io::Result<TcpStream> {
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect(&addr)?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(15)));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(1);
    let key = base64::engine::general_purpose::STANDARD.encode(nanos.to_le_bytes());
    let req = format!(
        "GET {path_and_query} HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Authorization: Bearer {bearer_token}\r\n\r\n"
    );
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    // Read HTTP response headers byte-by-byte until \r\n\r\n so we don't consume WS frames.
    let mut header_buf = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    while header_buf.len() < 8192 {
        stream.read_exact(&mut byte)?;
        header_buf.push(byte[0]);
        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&header_buf);
    if !head.starts_with("HTTP/1.1 101") && !head.starts_with("HTTP/1.0 101") {
        let first = head.lines().next().unwrap_or("Unexpected response");
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("Cloud RPC WebSocket upgrade failed: {first}"),
        ));
    }
    let _ = stream.set_read_timeout(None);
    Ok(stream)
}

pub fn read_message(stream: &mut impl Read) -> io::Result<WsMessage> {
    let mut assembled = Vec::new();
    let mut initial_opcode: Option<u8> = None;

    loop {
        let mut header = [0u8; 2];
        stream.read_exact(&mut header)?;
        let fin = (header[0] & 0x80) != 0;
        let opcode = header[0] & 0x0F;
        let masked = (header[1] & 0x80) != 0;
        let mut len = (header[1] & 0x7F) as u64;

        if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext)?;
            len = u16::from_be_bytes(ext) as u64;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            stream.read_exact(&mut ext)?;
            len = u64::from_be_bytes(ext);
        }

        if len as usize > MAX_WS_PAYLOAD
            || assembled.len().saturating_add(len as usize) > MAX_WS_PAYLOAD
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "WebSocket payload exceeds limit",
            ));
        }

        let mask = if masked {
            let mut m = [0u8; 4];
            stream.read_exact(&mut m)?;
            Some(m)
        } else {
            None
        };

        let mut payload = vec![0u8; len as usize];
        stream.read_exact(&mut payload)?;
        if let Some(m) = mask {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= m[i % 4];
            }
        }

        match opcode {
            0x8 => return Ok(WsMessage::Close),
            0x9 => return Ok(WsMessage::Ping(payload)),
            0xA => return Ok(WsMessage::Pong(payload)),
            0x1 | 0x2 => {
                if initial_opcode.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Unexpected new data frame during fragmentation",
                    ));
                }
                initial_opcode = Some(opcode);
                assembled.extend_from_slice(&payload);
                if fin {
                    let text = String::from_utf8(assembled).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8 in WS message")
                    })?;
                    return Ok(WsMessage::Text(text));
                }
            }
            0x0 => {
                if initial_opcode.is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Unexpected continuation frame",
                    ));
                }
                assembled.extend_from_slice(&payload);
                if fin {
                    let text = String::from_utf8(assembled).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8 in WS message")
                    })?;
                    return Ok(WsMessage::Text(text));
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unsupported WebSocket opcode",
                ));
            }
        }
    }
}
