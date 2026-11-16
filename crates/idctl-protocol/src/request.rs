//! Vendor control transfers understood by iD mk3 firmware.
//!
//! Every request is a vendor-class transfer on endpoint 0 with interface
//! recipient; the firmware also answers device recipient, which is what a
//! client sharing the interface with the kernel audio driver has to use.

/// Address of one parameter inside the device's control tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParamAddr {
    /// Entity type (selects the block: system, monitor, input, mixer, ...).
    pub kind: u8,
    /// Instance within the type (channel, bus, band, ...).
    pub inst: u8,
    /// Parameter within the instance.
    pub param: u8,
}

impl ParamAddr {
    pub const fn new(kind: u8, inst: u8, param: u8) -> Self {
        Self { kind, inst, param }
    }

    const fn value(self) -> u16 {
        (self.param as u16) << 8 | self.inst as u16
    }

    const fn index(self, iface: u8) -> u16 {
        (self.kind as u16) << 8 | iface as u16
    }
}

/// Transfer direction, from the host's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

/// `bRequest` values.
mod code {
    pub const PARAM: u8 = 1;
    pub const PENDING_CHANGES: u8 = 3;
    pub const METERS: u8 = 4;
    pub const COMMAND: u8 = 5;
    pub const MEMORY: u8 = 7;
}

/// A control transfer ready to hand to a USB backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub direction: Direction,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    /// Payload for `Out` requests; empty for `In`.
    pub data: Vec<u8>,
    /// Bytes to read for `In` requests; zero for `Out`.
    pub length: u16,
}

impl Request {
    /// Read `length` bytes of a parameter.
    pub fn get_param(iface: u8, addr: ParamAddr, length: u16) -> Self {
        Self::get(code::PARAM, addr.value(), addr.index(iface), length)
    }

    /// Write a parameter value (little-endian encoded by the caller).
    pub fn set_param(iface: u8, addr: ParamAddr, data: &[u8]) -> Self {
        Self::set(code::PARAM, addr.value(), addr.index(iface), data)
    }

    /// Drain the queue of parameters changed since the last read.
    pub fn pending_changes(iface: u8) -> Self {
        Self::get(code::PENDING_CHANGES, 0, u16::from(iface), 1023)
    }

    /// Read `count` 16-bit meter levels of `group`.
    pub fn meters(iface: u8, group: u16, count: u16) -> Self {
        Self::get(code::METERS, group, u16::from(iface), count * 2)
    }

    /// Send a command message.
    pub fn command(iface: u8, command: u16, data: &[u8]) -> Self {
        Self::set(code::COMMAND, command, u16::from(iface), data)
    }

    /// Write to device memory (blob parameters such as filter coefficients).
    pub fn mem_write(iface: u8, addr: ParamAddr, data: &[u8]) -> Self {
        Self::set(code::MEMORY, addr.value(), addr.index(iface), data)
    }

    fn get(request: u8, value: u16, index: u16, length: u16) -> Self {
        Self {
            direction: Direction::In,
            request,
            value,
            index,
            data: Vec::new(),
            length,
        }
    }

    fn set(request: u8, value: u16, index: u16, data: &[u8]) -> Self {
        Self {
            direction: Direction::Out,
            request,
            value,
            index,
            data: data.to_vec(),
            length: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_fields_pack_like_the_vendor_app() {
        // Monitor (type 4) parameter 0x12, instance 0, interface 0.
        let r = Request::get_param(0, ParamAddr::new(4, 0, 0x12), 1);
        assert_eq!(
            (r.request, r.value, r.index, r.length),
            (1, 0x1200, 0x0400, 1)
        );

        let r = Request::set_param(2, ParamAddr::new(5, 3, 0x0a), &[1]);
        assert_eq!((r.request, r.value, r.index), (1, 0x0a03, 0x0502));
        assert_eq!(r.data, [1]);
    }

    #[test]
    fn mem_write_uses_request_seven() {
        let r = Request::mem_write(0, ParamAddr::new(7, 3, 0x1d), &[1, 2]);
        assert_eq!(r.direction, Direction::Out);
        assert_eq!((r.request, r.value, r.index), (7, 0x1d03, 0x0700));
        assert_eq!(r.data, [1, 2]);
    }

    #[test]
    fn meters_read_two_bytes_per_level() {
        let r = Request::meters(0, 0, 26);
        assert_eq!((r.request, r.value, r.index, r.length), (4, 0, 0, 52));
    }
}
