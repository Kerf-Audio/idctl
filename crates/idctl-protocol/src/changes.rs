//! Decoding of the pending-change queue (`bRequest 3`).
//!
//! The device reports which parameters changed (hardware buttons, wheel, clock
//! events) as 4-byte records `(param, inst, 0, kind)`, ended by a record whose
//! first byte is `0xFF`. The host then re-reads each parameter it cares about.

use crate::request::ParamAddr;

const RECORD: usize = 4;
const END: u8 = 0xFF;

/// Parameters named in a pending-change reply, in device order.
pub fn parse_changes(reply: &[u8]) -> impl Iterator<Item = ParamAddr> + '_ {
    let (records, _) = reply.as_chunks::<RECORD>();
    records
        .iter()
        .take_while(|r| r[0] != END)
        .map(|r| ParamAddr::new(r[3], r[1], r[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_at_terminator_and_ignores_trailing_bytes() {
        let reply = [
            0x12, 0, 0, 4, 0x08, 0, 0, 0, 0xFF, 0xFF, 0, 0xFF, 0x55, 0x55, 0, 4,
        ];
        let got: Vec<_> = parse_changes(&reply).collect();
        assert_eq!(
            got,
            [ParamAddr::new(4, 0, 0x12), ParamAddr::new(0, 0, 0x08)]
        );
    }

    #[test]
    fn partial_trailing_record_is_dropped() {
        assert_eq!(parse_changes(&[1, 2, 0]).count(), 0);
    }
}
