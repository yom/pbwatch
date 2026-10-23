use std::time::Duration;

/// Why a session or connection attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// 10s connect timeout (counted as a failure — fix B5).
    ConnectTimeout,
    /// Firmware RFCOMM reset (bud placement change). Reconnect fast, no counter.
    Reset,
    /// BlueZ confirmed the device disconnected at the ACL level.
    #[allow(dead_code)]
    HostDown,
    /// BlueZ PropertiesChanged: Connected → false (step 8).
    #[allow(dead_code)]
    BluezDisconnected,
    /// All other I/O errors.
    Other,
}

/// What the main loop should do after a failure.
#[derive(Debug, Clone)]
pub struct Decision {
    /// Fire on_disconnect this cycle.
    pub announce_disconnect: bool,
    pub next: Next,
}

#[derive(Debug, Clone)]
pub enum Next {
    /// Retry after this delay.
    Retry(Duration),
    /// Wait for BlueZ to report Connected=true, then retry.
    Dormant,
}

/// Reconnection state machine — pure, no I/O, fully unit-tested.
#[derive(Default, Debug)]
pub struct Reconnect {
    drop_count:            u32,
    ever_connected:        bool,
    /// Guards against duplicate disconnect announcements within one session (fix B1/A7).
    disconnect_announced:  bool,
}

impl Reconnect {
    /// Called on every successful connect. Returns true if this is a reconnect
    /// (there was a prior successful session in this daemon run).
    pub fn on_connected(&mut self) -> bool {
        let was_reconnect = self.ever_connected;
        self.ever_connected       = true;
        self.drop_count           = 0;
        self.disconnect_announced = false;
        was_reconnect
    }

    /// Called when waking from dormant mode (fix B3: drop_count reset so the
    /// next pair of failures doesn't re-announce a disconnect).
    pub fn on_dormant_wake(&mut self) {
        self.drop_count = 0;
    }

    /// Classifies the failure and returns the action to take.
    pub fn on_failure(&mut self, f: Failure, bt_connected: bool) -> Decision {
        match f {
            // Fast transient failure — don't count, don't announce, don't go dormant.
            Failure::Reset => Decision {
                announce_disconnect: false,
                next: Next::Retry(Duration::from_millis(500)),
            },

            // Device genuinely gone (BlueZ confirmed).
            Failure::HostDown | Failure::BluezDisconnected if !bt_connected => {
                self.drop_count = 0;
                Decision {
                    announce_disconnect: self.try_announce(),
                    next: Next::Dormant,
                }
            }

            // Everything else: ConnectTimeout, ConnRefused, EOF, HostDown-while-connected, etc.
            _ => {
                self.drop_count += 1;
                let announce = self.drop_count >= 2 && self.try_announce();

                let next = if self.drop_count < 2 {
                    Next::Retry(Duration::from_secs(2))
                } else if bt_connected {
                    // BlueZ says connected but RFCOMM keeps failing — use exponential
                    // backoff instead of going dormant (fix B4: no hot spin).
                    Next::Retry(backoff(self.drop_count))
                } else {
                    Next::Dormant
                };

                Decision { announce_disconnect: announce, next }
            }
        }
    }

    /// Returns true and marks the announcement as done. Returns false if we
    /// have never connected (guards H1) or already announced this session.
    fn try_announce(&mut self) -> bool {
        if !self.ever_connected || self.disconnect_announced {
            return false;
        }
        self.disconnect_announced = true;
        true
    }
}

/// Returns `2^(drop_count-1)` seconds, capped at 30. Called with `drop_count ≥ 2`.
/// Effective sequence: 2, 4, 8, 16, 30s.
fn backoff(drop_count: u32) -> Duration {
    let secs = (2u64).saturating_pow(drop_count.saturating_sub(1)).min(30);
    Duration::from_secs(secs)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn retry_secs(d: &Decision) -> Option<u64> {
        match d.next {
            Next::Retry(dur) => Some(dur.as_secs()),
            Next::Dormant    => None,
        }
    }

    // H1: two startup failures with no prior connect → no announcement
    #[test]
    fn h1_no_spurious_disconnect_at_startup() {
        let mut m = Reconnect::default();
        let d1 = m.on_failure(Failure::Other, false);
        assert!(!d1.announce_disconnect);
        let d2 = m.on_failure(Failure::Other, false);
        assert!(!d2.announce_disconnect, "must not announce before ever_connected");
    }

    // Normal session: connect, fail twice → announce once
    #[test]
    fn announce_once_after_two_failures() {
        let mut m = Reconnect::default();
        m.on_connected();
        let d1 = m.on_failure(Failure::Other, false);
        assert!(!d1.announce_disconnect);
        let d2 = m.on_failure(Failure::Other, false);
        assert!(d2.announce_disconnect);
        // Third failure: no second announcement (A7)
        let d3 = m.on_failure(Failure::Other, false);
        assert!(!d3.announce_disconnect, "must not announce twice per session");
    }

    // A7: after dormant wake, fail twice → no second announcement
    #[test]
    fn a7_no_double_announce_after_dormant_wake() {
        let mut m = Reconnect::default();
        m.on_connected();
        m.on_failure(Failure::Other, false);
        let d = m.on_failure(Failure::Other, false);
        assert!(d.announce_disconnect); // first announcement

        // Dormant wake resets drop_count; disconnect_announced stays true
        m.on_dormant_wake();
        let d2 = m.on_failure(Failure::Other, false);
        assert!(!d2.announce_disconnect, "already announced this session");
        let d3 = m.on_failure(Failure::Other, false);
        assert!(!d3.announce_disconnect);
    }

    // B3: on_dormant_wake resets drop_count
    #[test]
    fn b3_dormant_wake_resets_drop_count() {
        let mut m = Reconnect::default();
        m.on_connected();
        m.on_failure(Failure::Other, true);
        m.on_failure(Failure::Other, true);
        m.on_dormant_wake();
        assert_eq!(m.drop_count, 0);
    }

    // B4: ConnectTimeout+BlueZ Connected → Retry with backoff, not Dormant
    #[test]
    fn b4_no_hot_spin_when_blz_connected() {
        let mut m = Reconnect::default();
        m.on_connected();
        for _ in 0..5 {
            let d = m.on_failure(Failure::ConnectTimeout, true /* bt_connected */);
            match d.next {
                Next::Dormant => panic!("must not go dormant while BlueZ says Connected"),
                Next::Retry(dur) => assert!(dur.as_secs() >= 2, "must use backoff"),
            }
        }
    }

    // B5: ConnectTimeout counted as failure
    #[test]
    fn b5_connect_timeout_counts_as_failure() {
        let mut m = Reconnect::default();
        m.on_connected();
        let d1 = m.on_failure(Failure::ConnectTimeout, false);
        assert_eq!(m.drop_count, 1);
        assert!(!d1.announce_disconnect);
        let d2 = m.on_failure(Failure::ConnectTimeout, false);
        assert_eq!(m.drop_count, 2);
        assert!(d2.announce_disconnect);
        assert!(matches!(d2.next, Next::Dormant));
    }

    // Reset: no counter bump, no announcement, fast retry
    #[test]
    fn reset_is_not_a_failure() {
        let mut m = Reconnect::default();
        m.on_connected();
        // main.rs always passes bt=true for Reset (device physically present)
        let d = m.on_failure(Failure::Reset, true);
        assert!(!d.announce_disconnect);
        assert_eq!(m.drop_count, 0);
        assert_eq!(retry_secs(&d), Some(0)); // 500ms rounds to 0 with as_secs
    }

    // HostDown with BlueZ confirming → Dormant + announce
    #[test]
    fn host_down_blz_confirms_goes_dormant() {
        let mut m = Reconnect::default();
        m.on_connected();
        let d = m.on_failure(Failure::HostDown, false /* bt not connected */);
        assert!(d.announce_disconnect);
        assert!(matches!(d.next, Next::Dormant));
        assert_eq!(m.drop_count, 0);
    }

    // HostDown with BlueZ still reporting Connected → counted as Other
    #[test]
    fn host_down_blz_says_connected_counts_as_other() {
        let mut m = Reconnect::default();
        m.on_connected();
        let d = m.on_failure(Failure::HostDown, true /* bt still connected */);
        assert_eq!(m.drop_count, 1);
        assert!(matches!(d.next, Next::Retry(_)));
    }

    // F12: first connect after startup failures must NOT be labeled "(reconnect)"
    #[test]
    fn f12_first_connect_after_startup_failure_is_not_reconnect() {
        let mut m = Reconnect::default();
        m.on_failure(Failure::Other, true); // startup failure
        let was_reconnect = m.on_connected();
        assert!(!was_reconnect, "first successful connect must not be labeled a reconnect");
    }

    // on_connected resets everything for next session
    #[test]
    fn reconnect_resets_state() {
        let mut m = Reconnect::default();
        m.on_connected();
        m.on_failure(Failure::Other, false);
        m.on_failure(Failure::Other, false); // announced
        m.on_connected();
        assert_eq!(m.drop_count, 0);
        assert!(!m.disconnect_announced);
    }

    // Backoff grows and caps at 30s
    #[test]
    fn backoff_caps_at_30s() {
        assert_eq!(backoff(1).as_secs(), 1);
        assert_eq!(backoff(2).as_secs(), 2);
        assert_eq!(backoff(3).as_secs(), 4);
        assert_eq!(backoff(4).as_secs(), 8);
        assert_eq!(backoff(5).as_secs(), 16);
        assert_eq!(backoff(6).as_secs(), 30);
        assert_eq!(backoff(100).as_secs(), 30);
    }
}
