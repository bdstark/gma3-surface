//! Versioned, bounded JSON datagrams for the fixed localhost-only transport.
//! No application-level authentication: any local process can use this protocol.
pub const MAGIC: &str = "MTX2";
pub const MAX_TO_PLUGIN: usize = 512;
pub const MAX_FROM_PLUGIN: usize = 16384;

pub fn frame(json: &str) -> Vec<u8> { format!("{MAGIC} {json}").into_bytes() }
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError { Oversized, BadMagic, BadFrame }
pub fn unframe(data: &[u8], max_len: usize) -> Result<&str, FrameError> {
    if data.len() > max_len { return Err(FrameError::Oversized); }
    let text = data.strip_prefix(b"MTX2 ").ok_or(FrameError::BadMagic)?;
    std::str::from_utf8(text).map_err(|_| FrameError::BadFrame)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_reject_legacy_and_malformed_frames() {
        assert_eq!(unframe(&frame("{}"), MAX_TO_PLUGIN), Ok("{}"));
        assert_eq!(unframe(b"MTX1 0000000000000000 {}", MAX_TO_PLUGIN), Err(FrameError::BadMagic));
        assert_eq!(unframe(b"OSC1 {}", MAX_TO_PLUGIN), Err(FrameError::BadMagic));
        assert_eq!(unframe(b"MTX2 \xff", MAX_TO_PLUGIN), Err(FrameError::BadFrame));
        assert_eq!(unframe(&vec![0;513], MAX_TO_PLUGIN), Err(FrameError::Oversized));
    }
}
