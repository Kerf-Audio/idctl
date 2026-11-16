//! Per-channel DSP the device cannot report back: EQ bands, variable phase
//! and the high-pass cutoff. The host keeps them and sends every change.
//! Pure: no USB, no iced.

use idctl_protocol::dsp::{self, Band, Write, USER_BANDS};

/// Channels with DSP: two analogue inputs, then eight optical.
pub const DSP_CHANNELS: usize = 10;
pub const HPF_DEFAULT_HZ: f32 = 100.0;

#[derive(Clone, Copy, Debug)]
pub struct ChannelDsp {
    pub eq: [Band; 4],
    pub eq_on: bool,
    pub phase_on: bool,
    pub phase_deg: f32,
    pub hpf_hz: f32,
}

impl Default for ChannelDsp {
    fn default() -> Self {
        Self {
            eq: USER_BANDS,
            eq_on: false,
            phase_on: false,
            phase_deg: 0.0,
            hpf_hz: HPF_DEFAULT_HZ,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DspEdit {
    EqOn(bool),
    /// Band index 0..4 and its new settings.
    Band(usize, Band),
    PhaseOn(bool),
    Phase(f32),
    /// New cutoff, and whether the high-pass filter is switched on.
    HpfHz(f32, bool),
}

impl ChannelDsp {
    /// Apply an edit on channel `ch`; returns the writes that carry it.
    pub fn edit(&mut self, ch: u8, edit: DspEdit) -> Vec<Write> {
        match edit {
            DspEdit::EqOn(on) => {
                self.eq_on = on;
                (0..self.eq.len())
                    .flat_map(|i| dsp::user_band(ch, i, &self.eq[i], on))
                    .collect()
            }
            DspEdit::Band(i, band) => {
                self.eq[i] = band;
                dsp::user_band(ch, i, &band, self.eq_on)
            }
            DspEdit::PhaseOn(on) => {
                self.phase_on = on;
                dsp::vari_phase(ch, self.phase_deg, on)
            }
            DspEdit::Phase(deg) => {
                self.phase_deg = deg;
                dsp::vari_phase(ch, deg, self.phase_on)
            }
            DspEdit::HpfHz(hz, on) => {
                self.hpf_hz = hz;
                dsp::hpf(ch, hz, on)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_edit_follows_eq_switch() {
        let mut d = ChannelDsp::default();
        let band = Band {
            gain_db: 4.0,
            ..USER_BANDS[1]
        };
        let off = d.edit(0, DspEdit::Band(1, band));
        d.edit(0, DspEdit::EqOn(true));
        let on = d.edit(0, DspEdit::Band(1, band));
        assert_eq!(off.first(), on.first(), "same control packet");
        assert_ne!(off[1..], on[1..], "bypass blobs while off");
        assert_eq!(off, dsp::user_band(0, 1, &band, false));
    }
}
