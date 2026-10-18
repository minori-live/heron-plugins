use super::*;
use truce::core::PluginRuntime;

#[test]
fn independent_coarse_and_fine_parameters_combine_without_clipping_the_endpoints() {
    let params = RepitchParams::default();
    assert_eq!(params.pitch_shift(), 0.0);
    params.set_plain(0, 3.0);
    params.set_plain(1, -25.0);
    assert_eq!(params.pitch_shift(), 2.75);
    params.set_plain(0, -12.0);
    params.set_plain(1, -100.0);
    assert_eq!(params.pitch_shift(), -13.0);
    params.set_plain(0, 12.0);
    params.set_plain(1, 100.0);
    assert_eq!(params.pitch_shift(), 13.0);
}

#[test]
fn repitch_has_its_own_effect_identity_and_persistent_pitch_controls() {
    let info = Plugin::info();
    assert_eq!(info.clap_id, "live.minori.heron.repitch");
    assert_eq!(info.au_type, *b"aufx");
    assert_eq!(info.fourcc, *b"HRRP");
    let params = RepitchParams::default();
    params.set_plain(0u32, -4.0);
    params.set_plain(1u32, 37.0);
    params.set_plain(2u32, 0.0);
    params.set_plain(3u32, 0.0);
    let (ids, values) = params.collect_values();
    let stored: Vec<_> = ids.into_iter().zip(values).collect();
    assert_eq!(stored, vec![(0, -4.0), (1, 37.0), (2, 0.0), (3, 0.0)]);
    let restored = RepitchParams::default();
    restored.restore_values(&stored);
    assert_eq!(restored.semitones.value(), -4);
    assert_eq!(restored.cents.value(), 37);
    assert!(!restored.formant.value());
    assert_eq!(restored.quality.value(), 0);
    assert!((restored.pitch_shift() + 3.63).abs() < 1e-6);
    assert!(!Arc::ptr_eq(&params.status, &restored.status));
}
