use pbcommon::{AncMode, BudsState};

// Sent on connect to request current ANC state.
pub const GET_STATE: [u8; 4] = [0x08, 0x11, 0x00, 0x00];

// ── Frame framing ─────────────────────────────────────────────────────────────

/// A decoded GFPS message frame.
#[derive(Debug, Clone)]
pub struct Frame {
    pub group: u8,
    pub code: u8,
    pub payload: Vec<u8>,
}

/// Accumulates raw bytes and yields complete frames.
///
/// Cancel-safe: bytes are only consumed from the internal buffer after a
/// complete frame is available. Never use `read_exact` inside `select!` —
/// use `read` + push to this buffer instead.
#[derive(Default)]
pub struct FrameBuf {
    buf: Vec<u8>,
}

impl FrameBuf {
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Returns the next complete frame if one is available, consuming it from
    /// the buffer. Returns None when more bytes are needed.
    pub fn try_next(&mut self) -> Option<Frame> {
        if self.buf.len() < 4 {
            return None;
        }
        let length = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
        let total = 4 + length;
        if self.buf.len() < total {
            return None;
        }
        let frame = Frame {
            group:   self.buf[0],
            code:    self.buf[1],
            payload: self.buf[4..total].to_vec(),
        };
        self.buf.drain(..total);
        Some(frame)
    }
}

// ── Message classification ────────────────────────────────────────────────────

pub enum Msg {
    /// Battery info: (left, right, case) bytes.
    Battery([u8; 3]),
    /// ANC state update.
    Anc { state: AncMode, supported: Vec<AncMode> },
    /// Model ID (3 bytes big-endian).
    ModelId(u32),
    /// Unhandled message (frame data discarded; use --debug to see raw bytes before classify).
    Other,
}

pub fn classify(frame: Frame) -> Msg {
    match (frame.group, frame.code, frame.payload.len()) {
        (0x03, 0x03, 3) => {
            Msg::Battery([frame.payload[0], frame.payload[1], frame.payload[2]])
        }
        (0x08, 0x13, n) if n >= 4 => {
            let (state, supported) = decode_anc(&frame.payload);
            Msg::Anc { state, supported }
        }
        (0x03, 0x01, n) if n >= 3 => {
            let id = u32::from_be_bytes([0, frame.payload[0], frame.payload[1], frame.payload[2]]);
            Msg::ModelId(id)
        }
        _ => { let _ = frame; Msg::Other }
    }
}

// ── Battery decoding ──────────────────────────────────────────────────────────

/// Decodes one battery byte from the GFPS BatteryInfo message.
///
/// Returns `(percentage, is_charging)`.
/// Bit 7 = charging flag. Bits 6-0 = percentage; `0x7f` means "not present"
/// (bud in case), represented as `None`. Values are clamped to 0-100.
pub fn decode_battery_byte(b: u8) -> (Option<u8>, bool) {
    let charging = (b & 0x80) != 0;
    let raw = b & 0x7f;
    let pct = if raw == 0x7f { None } else { Some(raw.min(100)) };
    (pct, charging)
}

/// Applies three battery bytes to BudsState fields.
pub fn apply_battery(state: &mut BudsState, bytes: [u8; 3]) {
    let (l, lc) = decode_battery_byte(bytes[0]);
    let (r, rc) = decode_battery_byte(bytes[1]);
    let (c, cc) = decode_battery_byte(bytes[2]);
    state.left            = l;
    state.right           = r;
    state.case            = c;
    state.left_charging   = lc;
    state.right_charging  = rc;
    state.case_charging   = cc;
}

// ── ANC decoding ──────────────────────────────────────────────────────────────

// Priority order matches the original Python implementation.
const ANC_BITS: &[(u8, AncMode)] = &[
    (0x80, AncMode::Transparency),
    (0x40, AncMode::Adaptive),
    (0x20, AncMode::Off),
    (0x08, AncMode::NoiseCancellation),
];

/// Decodes an ANC state message payload (`p[1]` = supported mask, `p[3]` = active mask).
pub fn decode_anc(payload: &[u8]) -> (AncMode, Vec<AncMode>) {
    let state = ANC_BITS
        .iter()
        .find(|(bit, _)| payload[3] & bit != 0)
        .map(|(_, mode)| *mode)
        .unwrap_or(AncMode::Unknown);

    let supported = ANC_BITS
        .iter()
        .filter(|(bit, _)| payload[1] & bit != 0)
        .map(|(_, mode)| *mode)
        .collect();

    (state, supported)
}

// ── Model ID → sprite set + name ──────────────────────────────────────────────

pub fn model_info(model_id: u32) -> Option<(&'static str, &'static str)> {
    // (sprite_set, human_name)
    // These are the 3-byte ModelId values from message group=0x03 code=0x01,
    // distinct from the Fast Pair catalog IDs used for pairing.
    match model_id {
        0x6EDAF7 => Some(("pro",  "Pixel Buds Pro")),
        // Source: github.com/tedsluis/opencontrolpixelbudspro2 PROTOCOL.md
        0xDA2DB1 => Some(("pro2", "Pixel Buds Pro 2")),
        // A-Series appears not to send a 0x03/0x01 ModelId message.
        _ => None,
    }
}

pub fn sprite_set_for(model_id: Option<u32>) -> &'static str {
    model_id
        .and_then(|id| model_info(id).map(|(set, _)| set))
        .unwrap_or("default")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Battery byte decoding

    #[test]
    fn battery_not_present() {
        assert_eq!(decode_battery_byte(0x7f), (None, false));
    }

    #[test]
    fn battery_not_present_and_charging() {
        // 0xff = bit7 set + value 0x7f = absent but charging flag true
        assert_eq!(decode_battery_byte(0xff), (None, true));
    }

    #[test]
    fn battery_normal() {
        assert_eq!(decode_battery_byte(0x55), (Some(85), false));
    }

    #[test]
    fn battery_charging() {
        assert_eq!(decode_battery_byte(0xd5), (Some(85), true));
    }

    #[test]
    fn battery_clamp_at_100() {
        // 0x7e = 126, should clamp to 100
        assert_eq!(decode_battery_byte(0x7e), (Some(100), false));
        // 101
        assert_eq!(decode_battery_byte(0x65), (Some(100), false));
    }

    #[test]
    fn battery_zero() {
        assert_eq!(decode_battery_byte(0x00), (Some(0), false));
    }

    // ANC decoding

    #[test]
    fn anc_noise_cancellation() {
        let p = [0, 0x08, 0, 0x08];
        let (state, supported) = decode_anc(&p);
        assert_eq!(state, AncMode::NoiseCancellation);
        assert_eq!(supported, vec![AncMode::NoiseCancellation]);
    }

    #[test]
    fn anc_priority_transparency_wins() {
        // Both transparency (0x80) and NC (0x08) bits set — transparency wins
        let p = [0, 0xff, 0, 0x88];
        let (state, _) = decode_anc(&p);
        assert_eq!(state, AncMode::Transparency);
    }

    #[test]
    fn anc_unknown_no_match() {
        let p = [0, 0, 0, 0x00];
        let (state, supported) = decode_anc(&p);
        assert_eq!(state, AncMode::Unknown);
        assert!(supported.is_empty());
    }

    #[test]
    fn anc_supported_order() {
        // All bits set — order must be: Transparency, Adaptive, Off, NC
        let p = [0, 0xe8, 0, 0x80];
        let (_, supported) = decode_anc(&p);
        assert_eq!(supported, vec![
            AncMode::Transparency,
            AncMode::Adaptive,
            AncMode::Off,
            AncMode::NoiseCancellation,
        ]);
    }

    // FrameBuf

    #[test]
    fn framebuf_complete_frame() {
        let mut buf = FrameBuf::default();
        // group=0x03, code=0x03, length=3, payload=[85, 87, 34]
        buf.push(&[0x03, 0x03, 0x00, 0x03, 85, 87, 34]);
        let frame = buf.try_next().unwrap();
        assert_eq!(frame.group, 0x03);
        assert_eq!(frame.code, 0x03);
        assert_eq!(frame.payload, vec![85, 87, 34]);
        assert!(buf.try_next().is_none());
    }

    #[test]
    fn framebuf_split_header() {
        let mut buf = FrameBuf::default();
        buf.push(&[0x03, 0x03]);
        assert!(buf.try_next().is_none());
        buf.push(&[0x00, 0x03, 85, 87, 34]);
        let frame = buf.try_next().unwrap();
        assert_eq!(frame.payload, vec![85, 87, 34]);
    }

    #[test]
    fn framebuf_two_frames_in_one_push() {
        let mut buf = FrameBuf::default();
        // Two battery frames back to back
        buf.push(&[
            0x03, 0x03, 0x00, 0x03, 85, 87, 34,
            0x03, 0x03, 0x00, 0x03, 90, 92, 50,
        ]);
        let f1 = buf.try_next().unwrap();
        let f2 = buf.try_next().unwrap();
        assert_eq!(f1.payload, vec![85, 87, 34]);
        assert_eq!(f2.payload, vec![90, 92, 50]);
        assert!(buf.try_next().is_none());
    }

    #[test]
    fn framebuf_zero_length_payload() {
        let mut buf = FrameBuf::default();
        // group=0x08, code=0x11, length=0 — the GET_STATE response or echo
        buf.push(&[0x08, 0x11, 0x00, 0x00]);
        let frame = buf.try_next().unwrap();
        assert_eq!(frame.group, 0x08);
        assert_eq!(frame.code, 0x11);
        assert!(frame.payload.is_empty());
    }

    #[test]
    fn framebuf_partial_payload() {
        let mut buf = FrameBuf::default();
        buf.push(&[0x03, 0x03, 0x00, 0x03, 85]); // only 1 of 3 payload bytes
        assert!(buf.try_next().is_none());
        buf.push(&[87, 34]);
        assert!(buf.try_next().is_some());
    }

    // classify

    #[test]
    fn classify_battery_frame() {
        let frame = Frame { group: 0x03, code: 0x03, payload: vec![0x55, 0xd7, 0x22] };
        match classify(frame) {
            Msg::Battery([l, r, c]) => {
                assert_eq!(l, 0x55);
                assert_eq!(r, 0xd7);
                assert_eq!(c, 0x22);
            }
            _ => panic!("expected Msg::Battery"),
        }
    }

    #[test]
    fn classify_model_id_frame() {
        let frame = Frame { group: 0x03, code: 0x01, payload: vec![0x6E, 0xDA, 0xF7] };
        match classify(frame) {
            Msg::ModelId(id) => assert_eq!(id, 0x6EDAF7),
            _ => panic!("expected Msg::ModelId"),
        }
    }

    #[test]
    fn classify_anc_frame() {
        // payload[1]=0x08 (NC supported), payload[3]=0x08 (NC active)
        let frame = Frame { group: 0x08, code: 0x13, payload: vec![0, 0x08, 0, 0x08] };
        match classify(frame) {
            Msg::Anc { state, .. } => assert_eq!(state, AncMode::NoiseCancellation),
            _ => panic!("expected Msg::Anc"),
        }
    }

    #[test]
    fn classify_other_frame() {
        let frame = Frame { group: 0xFF, code: 0xFF, payload: vec![] };
        assert!(matches!(classify(frame), Msg::Other));
    }
}
