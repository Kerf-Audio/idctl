//! Value encodings used on the wire. All integers are little-endian.

/// Decibels carried as `i16` in 1/256 dB steps. `i16::MIN` (-128 dB) is "off".
pub const DB_SCALE: f32 = 256.0;

pub fn db_to_raw(db: f32) -> i16 {
    // `round` rounds half away from zero; the `as` cast saturates.
    (db * DB_SCALE).round() as i16
}

pub fn raw_to_db(raw: i16) -> f32 {
    f32::from(raw) / DB_SCALE
}

pub fn encode_db(db: f32) -> [u8; 2] {
    db_to_raw(db).to_le_bytes()
}

pub fn decode_db(data: &[u8]) -> Option<f32> {
    Some(raw_to_db(i16::from_le_bytes(
        data.get(..2)?.try_into().ok()?,
    )))
}

/// Normalised 0..=1 values (reverb size and level) travel as `value * 32767`.
pub fn encode_unit(v: f32) -> [u8; 2] {
    ((v.clamp(0.0, 1.0) * 32767.0).round() as u16).to_le_bytes()
}

pub fn decode_unit(data: &[u8]) -> Option<f32> {
    Some(f32::from(u16::from_le_bytes(data.get(..2)?.try_into().ok()?)) / 32767.0)
}

pub fn decode_u32(data: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(..4)?.try_into().ok()?))
}

/// Meter level in dBFS, `None` for silence. The scale is inferred (linear
/// amplitude, full scale = `u16::MAX`): silence reads 0 on a live unit, but a
/// calibrated signal has not yet been measured.
pub fn meter_db(raw: u16) -> Option<f32> {
    (raw > 0).then(|| 20.0 * (f32::from(raw) / f32::from(u16::MAX)).log10())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trips_and_rounds_half_away() {
        assert_eq!(db_to_raw(-12.0), -3072);
        assert_eq!(raw_to_db(-3072), -12.0);
        assert_eq!(db_to_raw(0.001953125), 1); // 0.5/256 rounds up
        assert_eq!(db_to_raw(-0.001953125), -1);
        assert_eq!(db_to_raw(-200.0), i16::MIN);
    }

    #[test]
    fn device_defaults_decode() {
        // Reverb defaults read from a real iD2+: 0.6, 8000 Hz, 0.8.
        assert!((decode_unit(&[0xcc, 0x4c]).unwrap() - 0.6).abs() < 1e-4);
        assert!((decode_unit(&[0x65, 0x66]).unwrap() - 0.8).abs() < 1e-3);
        assert_eq!(decode_u32(&[0x44, 0xac, 0, 0]), Some(44100));
        assert_eq!(decode_db(&[0x00, 0x80]), Some(-128.0));
        assert_eq!(decode_db(&[1]), None);
    }
}
