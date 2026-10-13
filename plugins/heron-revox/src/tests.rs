use super::*;
use truce::core::PluginRuntime;

#[test]
fn independent_coarse_and_fine_parameters_combine_without_clipping_the_endpoints() {
    let params = RevoxParams::default();
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
fn revox_has_its_own_effect_identity_and_two_persistent_pitch_controls() {
    let info = Plugin::info();
    assert_eq!(info.clap_id, "live.minori.heron.revox");
    assert_eq!(info.au_type, *b"aufx");
    assert_eq!(info.fourcc, *b"HRRV");
    let params = RevoxParams::default();
    params.set_plain(0, -4.0);
    params.set_plain(1, 37.0);
    let (ids, values) = params.collect_values();
    let stored: Vec<_> = ids.into_iter().zip(values).collect();
    assert_eq!(stored, vec![(0, -4.0), (1, 37.0)]);
    let restored = RevoxParams::default();
    restored.restore_values(&stored);
    assert_eq!(restored.semitones.value(), -4);
    assert_eq!(restored.cents.value(), 37);
    assert!((restored.pitch_shift() + 3.63).abs() < 1e-6);
    assert!(!Arc::ptr_eq(&params.status, &restored.status));
}

#[test]
#[ignore = "runs embedded FCPE/Revox graphs and the complete offline plugin path"]
fn embedded_models_render_the_combined_pitch_through_the_plugin_worker() {
    use std::time::Duration;
    use truce_test::{InputSource, driver};
    let signal: Vec<f32> = (0..192_000)
        .map(|sample| {
            let phase = std::f64::consts::TAU * 220.0 * f64::from(sample) / 48_000.0;
            (0.15 * phase.sin() + 0.07 * (2.0 * phase).sin() + 0.04 * (3.0 * phase).sin()) as f32
        })
        .collect();
    let result = driver!(Plugin)
        .sample_rate(48_000.0)
        .channels(1)
        .block_size(257)
        .process_mode(ProcessMode::Offline)
        .duration(Duration::from_secs(4))
        .input(InputSource::Buffer(vec![signal]))
        .setup(|plugin, _| {
            plugin.params().set_plain(0, 3.0);
            plugin.params().set_plain(1, 50.0);
        })
        .run();
    assert_eq!(
        result.plugin.params().status.state(),
        ProcessingState::Ready
    );
    let latency = result.plugin.latency() as usize;
    assert_eq!(latency, stream::Geometry::new(48_000.0).unwrap().latency);
    assert!(
        result.output[0][..latency]
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(result.output[0].iter().all(|sample| sample.is_finite()));
    let output = &result.output[0][latency + 24_000..latency + 48_000];
    let correlation = |lag: usize| {
        let mut xy = 0.0_f64;
        let mut xx = 0.0_f64;
        let mut yy = 0.0_f64;
        for index in 0..output.len() - 320 {
            let a = f64::from(output[index]);
            let b = f64::from(output[index + lag]);
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        xy / (xx * yy).sqrt()
    };
    let lag = (140..320)
        .max_by(|a, b| correlation(*a).total_cmp(&correlation(*b)))
        .unwrap();
    let hz = 48_000.0 / lag as f64;
    let expected = 220.0 * 2.0_f64.powf(3.5 / 12.0);
    let cents = 1200.0 * (hz / expected).log2();
    assert!(
        cents.abs() < 50.0,
        "expected {expected} Hz, measured {hz} Hz ({cents} cents)"
    );
}
