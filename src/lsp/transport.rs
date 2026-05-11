//! JSON-RPC framing over `Content-Length`-delimited messages.
//!
//! Both LSP requests and responses share the same on-the-wire format:
//!
//! ```text
//! Content-Length: N\r\n
//! \r\n
//! <N bytes of UTF-8 JSON>
//! ```

use std::io::{self, BufRead, BufReader, Read, Write};

/// Read one framed message off `r`. Returns the raw JSON bytes (no header).
pub fn read_message<R: Read>(r: &mut BufReader<R>) -> io::Result<Vec<u8>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "lsp transport closed",
            ));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // blank line — end of headers
        }
        if let Some(v) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(v.trim().parse::<usize>().map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, e)
            })?);
        }
        // Other headers (Content-Type, etc.) are ignored.
    }
    let len = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length")
    })?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Write a JSON payload framed with the proper `Content-Length` header.
pub fn write_message<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n", payload.len())?;
    w.write_all(payload)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_message() {
        let payload = br#"{"jsonrpc":"2.0","method":"test"}"#;
        let mut out = Vec::new();
        write_message(&mut out, payload).unwrap();
        let mut reader = BufReader::new(Cursor::new(out));
        let got = read_message(&mut reader).unwrap();
        assert_eq!(got, payload);
    }

    #[test]
    fn handles_extra_headers() {
        let mut data: Vec<u8> = Vec::new();
        let payload = br#"{"jsonrpc":"2.0"}"#;
        write!(&mut data, "Content-Length: {}\r\n", payload.len()).unwrap();
        write!(&mut data, "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n").unwrap();
        write!(&mut data, "\r\n").unwrap();
        data.extend_from_slice(payload);
        let mut reader = BufReader::new(Cursor::new(data));
        let got = read_message(&mut reader).unwrap();
        assert_eq!(got, payload);
    }
}
