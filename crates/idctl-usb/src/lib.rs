//! rusb transport. Sends [`idctl_protocol::Request`]s to an iD2+.
//!
//! No interface is claimed: the kernel audio driver owns interfaces 0-2, and
//! the firmware accepts the vendor requests with device recipient.

use std::time::Duration;

use idctl_protocol::{codec, params, Direction, ParamAddr, Request};
use rusb::{request_type, Device, DeviceHandle, GlobalContext, Recipient, RequestType};

pub const VENDOR_ID: u16 = 0x2708;
pub const PRODUCT_ID_ID2_PLUS: u16 = 0x0016;
const TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no iD2+ found")]
    NotFound,
    #[error("usb: {0}")]
    Usb(#[from] rusb::Error),
    #[error("short reply: wanted {wanted} bytes, got {got}")]
    Short { wanted: usize, got: usize },
}

pub struct Device2Plus {
    handle: DeviceHandle<GlobalContext>,
}

impl Device2Plus {
    /// Open the first attached iD2+.
    pub fn open() -> Result<Self, Error> {
        let dev: Device<GlobalContext> = rusb::devices()?
            .iter()
            .find(|d| {
                d.device_descriptor().is_ok_and(|n| {
                    n.vendor_id() == VENDOR_ID && n.product_id() == PRODUCT_ID_ID2_PLUS
                })
            })
            .ok_or(Error::NotFound)?;
        Ok(Self {
            handle: dev.open()?,
        })
    }

    /// Run one request; returns the reply bytes for `In` requests.
    pub fn transfer(&self, req: &Request) -> Result<Vec<u8>, Error> {
        match req.direction {
            Direction::In => {
                let rt = request_type(rusb::Direction::In, RequestType::Vendor, Recipient::Device);
                let mut buf = vec![0; usize::from(req.length)];
                let n = self.handle.read_control(
                    rt,
                    req.request,
                    req.value,
                    req.index,
                    &mut buf,
                    TIMEOUT,
                )?;
                buf.truncate(n);
                Ok(buf)
            }
            Direction::Out => {
                let rt = request_type(rusb::Direction::Out, RequestType::Vendor, Recipient::Device);
                self.handle.write_control(
                    rt,
                    req.request,
                    req.value,
                    req.index,
                    &req.data,
                    TIMEOUT,
                )?;
                Ok(Vec::new())
            }
        }
    }

    /// Read exactly `len` bytes of a parameter.
    pub fn get(&self, addr: ParamAddr, len: u16) -> Result<Vec<u8>, Error> {
        let data = self.transfer(&Request::get_param(0, addr, len))?;
        if data.len() < usize::from(len) {
            return Err(Error::Short {
                wanted: len.into(),
                got: data.len(),
            });
        }
        Ok(data)
    }

    pub fn get_u8(&self, addr: ParamAddr) -> Result<u8, Error> {
        Ok(self.get(addr, 1)?[0])
    }

    pub fn get_db(&self, addr: ParamAddr) -> Result<f32, Error> {
        let d = self.get(addr, 2)?;
        Ok(codec::raw_to_db(i16::from_le_bytes([d[0], d[1]])))
    }

    pub fn set(&self, addr: ParamAddr, data: &[u8]) -> Result<(), Error> {
        self.transfer(&Request::set_param(0, addr, data)).map(drop)
    }

    /// Write a blob to device memory (filter coefficients).
    pub fn mem_write(&self, addr: ParamAddr, data: &[u8]) -> Result<(), Error> {
        self.transfer(&Request::mem_write(0, addr, data)).map(drop)
    }

    pub fn set_u8(&self, addr: ParamAddr, v: u8) -> Result<(), Error> {
        self.set(addr, &[v])
    }

    pub fn set_db(&self, addr: ParamAddr, db: f32) -> Result<(), Error> {
        self.set(addr, &codec::encode_db(db))
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> Result<u32, Error> {
        Ok(codec::decode_u32(&self.get(params::system::SAMPLE_RATE, 4)?).unwrap_or(0))
    }

    /// Parameters changed on the device since the last call.
    pub fn pending_changes(&self) -> Result<Vec<ParamAddr>, Error> {
        let reply = self.transfer(&Request::pending_changes(0))?;
        Ok(idctl_protocol::parse_changes(&reply).collect())
    }

    /// Raw 16-bit levels of a meter group.
    pub fn meters(&self, group: u16, count: u16) -> Result<Vec<u16>, Error> {
        let d = self.transfer(&Request::meters(0, group, count))?;
        Ok(d.as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect())
    }
}
