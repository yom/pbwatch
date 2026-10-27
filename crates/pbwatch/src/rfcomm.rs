use std::io;
use std::time::Duration;

use anyhow::Context;
use bluer::rfcomm::{Security, SecurityLevel, Socket, SocketAddr, Stream};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

use pbwatch::protocol::{Frame, FrameBuf, GET_STATE, classify, Msg};
use pbcommon::BudsState;

const BURST_TIMEOUT:   Duration = Duration::from_millis(400);
const BURST_MAX:       Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ConnectError {
    Timeout,
    Io(io::Error),
}

impl std::error::Error for ConnectError {}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout  => write!(f, "connect timed out"),
            Self::Io(e)    => write!(f, "{e}"),
        }
    }
}

/// How a session ended — used by the reconnect state machine.
#[derive(Debug)]
pub enum SessionEnd {
    /// Remote firmware reset (bud placement change). Reconnect quickly (0.5s).
    Reset,
    /// Clean EOF or generic I/O error.
    Other(io::Error),
}

impl From<io::Error> for SessionEnd {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::ConnectionReset => SessionEnd::Reset,
            _ => SessionEnd::Other(e),
        }
    }
}

// ── Connection ────────────────────────────────────────────────────────────────

/// Parses and normalises a MAC address string to uppercase.
/// Accepts both `ab:cd:ef:01:23:45` and `AB:CD:EF:01:23:45`.
pub fn parse_mac(s: &str) -> anyhow::Result<bluer::Address> {
    s.to_uppercase()
        .parse::<bluer::Address>()
        .with_context(|| format!("invalid Bluetooth MAC address: {s:?}"))
}

/// Opens an RFCOMM connection on channel 2 with a 10s timeout.
///
/// Requires BT_SECURITY_MEDIUM so the kernel rejects connections from
/// devices that don't have a stored link-key (i.e. aren't bonded).
pub async fn connect(addr: bluer::Address) -> Result<Stream, ConnectError> {
    let socket = Socket::new().map_err(ConnectError::Io)?;
    socket.set_security(Security { level: SecurityLevel::Medium, key_size: 0 })
        .map_err(ConnectError::Io)?;
    let sa = SocketAddr::new(addr, 2);
    timeout(CONNECT_TIMEOUT, socket.connect(sa))
        .await
        .map_err(|_| ConnectError::Timeout)?
        .map_err(ConnectError::Io)
}

// ── Frame reader ──────────────────────────────────────────────────────────────

/// Wraps any `AsyncRead` and yields complete GFPS frames.
///
/// Cancel-safe: bytes enter the buffer only after `read()` returns.
/// Do NOT use `read_exact` inside `select!` — it is not cancel-safe.
pub struct FrameReader<R> {
    inner: R,
    buf:   FrameBuf,
    tmp:   [u8; 512],
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner, buf: FrameBuf::default(), tmp: [0u8; 512] }
    }

    pub async fn next(&mut self) -> io::Result<Frame> {
        loop {
            if let Some(frame) = self.buf.try_next() {
                return Ok(frame);
            }
            let n = self.inner.read(&mut self.tmp).await?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            self.buf.push(&self.tmp[..n]);
        }
    }
}

// ── Burst drain ───────────────────────────────────────────────────────────────

/// Drains the initial message burst sent by the device on connect.
///
/// Reads battery, ANC, and ModelId messages for up to 400ms (or 3s total), applying
/// each to `state`. Does not fire any triggers or print anything —
/// that happens in the main loop.
pub async fn drain_burst<R: AsyncRead + Unpin>(
    reader: &mut FrameReader<R>,
    state: &mut BudsState,
    debug: bool,
) {
    let deadline = tokio::time::Instant::now() + BURST_MAX;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }

        let per_msg = remaining.min(BURST_TIMEOUT);
        match timeout(per_msg, reader.next()).await {
            Err(_elapsed) => break,          // quiet for this window
            Ok(Err(_))    => break,          // connection closed during burst
            Ok(Ok(frame)) => {
                if debug {
                    log::debug!(
                        "  burst group=0x{:02x} code=0x{:02x} len={} payload={}",
                        frame.group, frame.code, frame.payload.len(),
                        hex::encode(&frame.payload)
                    );
                }
                match classify(frame) {
                    Msg::Battery(bytes)             => crate::protocol::apply_battery(state, bytes),
                    Msg::Anc { state: s, supported } => {
                        if s != pbcommon::AncMode::Unknown {
                            state.anc           = s;
                            state.anc_supported = supported;
                        }
                    }
                    Msg::ModelId(id) => {
                        state.model_id = Some(id);
                        if let Some((_, name)) = crate::protocol::model_info(id) {
                            state.model_name = Some(name.to_string());
                        }
                        if debug {
                            log::debug!("  model_id=0x{:06x} name={:?}", id, state.model_name);
                        }
                    }
                    Msg::Other => {}
                }
            }
        }
    }
}

/// Sends the GET_STATE request to ask for the current ANC state.
pub async fn send_get_state<W: AsyncWrite + Unpin>(writer: &mut W) -> io::Result<()> {
    writer.write_all(&GET_STATE).await
}

// ── hex helper (avoids pulling in the `hex` crate) ───────────────────────────
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
