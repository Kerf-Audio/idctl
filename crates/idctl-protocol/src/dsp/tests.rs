use super::*;

fn words(w: &Write) -> [i32; 8] {
    let Write::Mem(addr, data) = w else {
        panic!("expected a MEM write");
    };
    assert_eq!((addr.kind, addr.param), (7, 0x1d));
    assert_eq!(data.len(), 32);
    std::array::from_fn(|i| i32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap()))
}

fn peak(gain_db: f32) -> Band {
    Band {
        kind: FilterType::Peak,
        hz: 1000.0,
        gain_db,
        q: 1.0,
    }
}

#[test]
fn fixed_point_truncates_and_saturates() {
    assert_eq!(fixed(1.0), 1 << 30);
    assert_eq!(fixed(-0.5), -(1 << 29));
    assert_eq!(fixed(0.9999999999), (1 << 30) - 1);
    assert_eq!(fixed(2.0), i32::MAX);
    assert_eq!(fixed(-2.0), i32::MIN);
    assert_eq!(fixed(100.0), i32::MAX);
    assert_eq!(fixed(-100.0), i32::MIN);
}

#[test]
fn user_band_emits_packet_and_four_blobs() {
    let band = Band {
        kind: FilterType::Peak,
        hz: 1000.0,
        gain_db: -3.0,
        q: 2.0,
    };
    let w = user_band(2, 1, &band, true);
    assert_eq!(w.len(), 5);
    let Write::Set(addr, p) = &w[0] else {
        panic!("expected a SET first");
    };
    assert_eq!(*addr, ParamAddr::new(7, 2, 11));
    assert_eq!(p, &[8, 0, 0xe8, 0x03, 0x00, 0xfd, 0x00, 0x02]);

    let rates: Vec<i32> = w[1..].iter().map(|b| words(b)[1]).collect();
    assert_eq!(rates, [44_100, 48_000, 88_200, 96_000]);
    for b in &w[1..] {
        assert_eq!(words(b)[0], 5);
    }

    let svf = Svf::of(&band, 48_000.0);
    let b = words(&w[2]);
    assert_eq!(b[2], fixed(svf.m[0] / 2.0));
    assert_eq!(b[3], fixed(svf.m[1] / 2.0));
    assert_eq!(b[5], fixed(svf.a[0]));
    assert_eq!(b[7], fixed(svf.a[2]));
}

#[test]
fn user_band_param_follows_index() {
    for (i, band) in USER_BANDS.iter().enumerate() {
        let Write::Set(addr, _) = &user_band(0, i, band, true)[0] else {
            panic!("expected a SET first");
        };
        assert_eq!(addr.param, 10 + i as u8);
    }
}

#[test]
fn disabled_band_blob_equals_bypass_but_keeps_its_type() {
    let band = peak(6.0);
    let off = user_band(0, 0, &band, false);
    let bypass = user_band(
        0,
        0,
        &Band {
            kind: FilterType::Bypass,
            ..band
        },
        true,
    );
    assert_eq!(off[1..], bypass[1..]);
    let Write::Set(_, p) = &off[0] else {
        panic!("expected a SET first");
    };
    assert_eq!(p[0], 8);
    assert_eq!(words(&off[1])[2..], [1 << 29, 0, 0, 0, 0, 0]);
}

#[test]
fn hpf_emits_six_blobs_for_band_zero() {
    let w = hpf(1, 500.0, true);
    assert_eq!(w.len(), 7);
    let Write::Set(addr, p) = &w[0] else {
        panic!("expected a SET first");
    };
    assert_eq!(*addr, ParamAddr::new(7, 1, 6));
    assert_eq!(p, &[3, 0, 250, 0, 0, 0, 181, 0]);
    let rates: Vec<i32> = w[1..].iter().map(|b| words(b)[1]).collect();
    assert_eq!(rates, [44_100, 48_000, 88_200, 96_000, 176_400, 192_000]);
    assert!(w[1..].iter().all(|b| words(b)[0] == 0));
    // High-pass: m0 = 1 halved, m2 = -1 halved.
    let b = words(&w[2]);
    assert_eq!((b[2], b[4]), (1 << 29, -(1 << 29)));
}

#[test]
fn vari_phase_scales_by_four_and_maps_angle_to_frequency() {
    assert!((phase_hz(90.0) - 16_000.0).abs() < 1e-2);
    assert!((phase_hz(180.0) - 1000.0).abs() < 1e-2);
    assert!((phase_hz(0.0) - 101_000.0).abs() < 1e-2);

    let w = vari_phase(0, 180.0, true);
    assert_eq!(w.len(), 7);
    let Write::Set(addr, p) = &w[0] else {
        panic!("expected a SET first");
    };
    assert_eq!(*addr, ParamAddr::new(7, 0, 7));
    assert_eq!(p, &[12, 0, 0xe8, 0x03, 0, 0, 0, 1]);
    let b = words(&w[1]);
    assert_eq!(b[0], 1);
    // All-pass m = (1, -2k, 0) with k = 1, divided by 4.
    assert_eq!((b[2], b[3], b[4]), (1 << 28, -(1 << 29), 0));
}

#[test]
fn vari_phase_saturates_frequency_and_passes_through_above_20k() {
    let w = vari_phase(0, 0.0, true);
    let Write::Set(_, p) = &w[0] else {
        panic!("expected a SET first");
    };
    assert_eq!(u16::from_le_bytes([p[2], p[3]]), u16::MAX);
    assert_eq!(words(&w[1])[2..], [1 << 28, 0, 0, 0, 0, 0]);
}

#[test]
fn vari_phase_off_clears_the_type() {
    let Write::Set(_, p) = &vari_phase(0, 180.0, false)[0] else {
        panic!("expected a SET first");
    };
    assert_eq!(p[0], 0);
}

#[test]
fn magnitude_matches_band_shapes() {
    let fs = 48_000.0;
    assert!((magnitude_db(&peak(6.0), 1000.0, fs) - 6.0).abs() < 0.01);
    assert!(magnitude_db(&peak(6.0), 50.0, fs).abs() < 0.5);
    assert!(magnitude_db(&peak(0.0), 3000.0, fs).abs() < 1e-6);

    let ls = Band {
        kind: FilterType::LowShelf,
        hz: 200.0,
        gain_db: 9.0,
        q: 0.707,
    };
    assert!((magnitude_db(&ls, 20.0, fs) - 9.0).abs() < 0.2);
    assert!(magnitude_db(&ls, 15_000.0, fs).abs() < 0.2);

    let hs = Band {
        kind: FilterType::HighShelf,
        hz: 4000.0,
        gain_db: -9.0,
        q: 0.707,
    };
    assert!((magnitude_db(&hs, 20_000.0, fs) + 9.0).abs() < 0.3);
    assert!(magnitude_db(&hs, 40.0, fs).abs() < 0.2);

    let bypass = Band {
        kind: FilterType::Bypass,
        ..peak(6.0)
    };
    assert_eq!(magnitude_db(&bypass, 1000.0, fs), 0.0);
}

#[test]
fn pass_filters_cut_where_expected() {
    let fs = 48_000.0;
    let hp = Band {
        kind: FilterType::HighPass,
        hz: 1000.0,
        gain_db: 0.0,
        q: 0.707,
    };
    assert!(magnitude_db(&hp, 50.0, fs) < -40.0);
    assert!((magnitude_db(&hp, 1000.0, fs) + 3.0).abs() < 0.1);
    assert!(magnitude_db(&hp, 10_000.0, fs).abs() < 0.1);
    let lp = Band {
        kind: FilterType::LowPass,
        ..hp
    };
    assert!(magnitude_db(&lp, 10_000.0, fs) < -30.0);
    let ap = Band {
        kind: FilterType::AllPass,
        hz: 1000.0,
        gain_db: 0.0,
        q: 1.0,
    };
    assert!(magnitude_db(&ap, 3000.0, fs).abs() < 1e-6);
}

#[test]
fn time_encoding_round_trips_and_clamps() {
    assert_eq!(comp::encode_time(10.0), 60u16.to_le_bytes());
    assert!((comp::decode_time(&comp::encode_time(10.0)) - 10.0).abs() < 1e-4);
    assert_eq!(comp::encode_time(0.0), 20u16.to_le_bytes());
    assert_eq!(comp::encode_time(1e6), 60_000u16.to_le_bytes());
}

#[test]
fn ratio_and_mix_encoding() {
    for r in [1.0, 2.0, 4.0, 8.0, 16.0] {
        assert_eq!(comp::decode_ratio(&comp::encode_ratio(r)), r);
    }
    assert_eq!(comp::encode_ratio(100.0), 4096u16.to_le_bytes());
    assert_eq!(comp::encode_mix(false, 1.0), [0, 0]);
    assert_eq!(comp::encode_mix(true, 1.0), 32767u16.to_le_bytes());
    assert_eq!(comp::encode_mix(true, 0.5), 16384u16.to_le_bytes());
}

#[test]
fn addresses_are_type_seven() {
    assert_eq!(comp::attack(3), ParamAddr::new(7, 3, 0));
    assert_eq!(comp::makeup(3), ParamAddr::new(7, 3, 0x0e));
    assert_eq!(order(9), ParamAddr::new(7, 9, 5));
}
