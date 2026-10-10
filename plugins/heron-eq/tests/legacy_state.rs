//! Historical EQ evidence from Heron 0.6.5 before source extraction.
//!
//! Fixtures were emitted by original commit
//! b8a5c9c76b25275b252dc98b1684a5a87281043f, not by this producer's EQ.
//! `fixtures/provenance.json` records the exact source and capture harness.

use std::time::Duration;

use heron_eq::Plugin;
use truce::core::PluginRuntime;
use truce::prelude::*;
use truce_test::{InputSource, driver};

const LEGACY_STATE: &[u8] = include_bytes!("fixtures/heron-0.6.5-nondefault.pluginstate");
const LEGACY_AUDIO: &[u8] = include_bytes!("fixtures/heron-0.6.5-nondefault.f32le");

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParameterContract {
    id: u32,
    default_plain: f64,
    unit: String,
    minimum: f64,
    maximum: f64,
    steps: Option<u32>,
    flags: u32,
    quarter_points: [f64; 3],
}

#[test]
fn released_eq_identity_editor_and_automation_schema_are_preserved() {
    let info = Plugin::info();
    assert_eq!(info.clap_id, "live.minori.heron.eq");
    assert_eq!(
        truce::core::state::vst3_cid(info.vst3_id),
        [
            0xd5, 0x41, 0x83, 0x8a, 0x36, 0xca, 0xc9, 0xb6, 0xa9, 0x57, 0x27, 0x88, 0xf4, 0x0e,
            0xbb, 0x9f,
        ],
    );
    assert_eq!(info.au_manufacturer, *b"HRON");
    assert_eq!(info.fourcc, *b"HREQ");
    assert_eq!(info.au_type, *b"aumf");
    assert!(info.accepts_midi_in, "EQ MIDI learn remains exposed");
    let ids: Vec<u32> =
        serde_json::from_str(include_str!("fixtures/heron-0.6.5-parameter-ids.json")).unwrap();
    assert_eq!(
        Plugin::param_infos_static()
            .iter()
            .map(|param| param.id)
            .collect::<Vec<_>>(),
        ids,
        "retired slots and the 24 stable band slots must not be renumbered",
    );
    let plugin = Plugin::create();
    let editor = plugin.editor_builder()(plugin.params_arc()).expect("EQ ships its native editor");
    assert_eq!(editor.size(), (1120, 760));
}

#[test]
fn new_eq_instances_keep_original_defaults_units_and_automation_ranges() {
    let expected: Vec<ParameterContract> =
        serde_json::from_str(include_str!("fixtures/heron-0.6.5-default-parameters.json")).unwrap();
    let plugin = Plugin::create();
    let infos = Plugin::param_infos_static();
    for expected in expected {
        let actual = infos.iter().find(|p| p.id == expected.id).unwrap();
        assert_eq!(
            actual.unit.as_str(),
            expected.unit,
            "unit for {}",
            expected.id
        );
        assert_eq!(
            actual.range.step_count().map(std::num::NonZeroU32::get),
            expected.steps
        );
        assert_eq!(
            actual.flags.bits(),
            expected.flags,
            "host flags for {}",
            expected.id
        );
        let matches = |actual: f64, expected_value: f64| {
            assert!(
                (actual - expected_value).abs() < 1.0e-10 * expected_value.abs().max(1.0),
                "parameter {}: {actual}, original {expected_value}",
                expected.id,
            );
        };
        matches(actual.default_plain, expected.default_plain);
        matches(
            plugin.params().get_plain(expected.id).unwrap(),
            expected.default_plain,
        );
        matches(actual.range.min(), expected.minimum);
        matches(actual.range.max(), expected.maximum);
        for (point, expected) in [0.25, 0.5, 0.75].into_iter().zip(expected.quarter_points) {
            matches(actual.range.denormalize(point), expected);
        }
    }
}

#[test]
fn restores_original_eq_state_midi_bindings_and_audio() {
    let restored = driver!(Plugin)
        .sample_rate(48000.0)
        .duration(Duration::from_millis(20))
        .state_blob(LEGACY_STATE.to_vec())
        .input(InputSource::Generator(Box::new(|frame, sample_rate| {
            let t = frame as f64 / sample_rate;
            (0.1 * (std::f64::consts::TAU * 440.0 * t).sin()
                + 0.05 * (std::f64::consts::TAU * 8000.0 * t).sin()) as f32
        })))
        .run();
    for (id, expected) in [
        (0, -3.0),
        (3, 0.8),
        (8, 0.2),
        (9, 1.0),
        (100, 1.0),
        (103, 1600.0),
        (104, 6.0),
        (105, 1.25),
        (836, 1.0),
        (839, 8000.0),
        (840, -9.0),
        (841, 0.75),
        (855, 1.0),
    ] {
        let actual = restored.plugin.params().get_plain(id).unwrap();
        assert!(
            (actual - expected).abs() < 0.0001,
            "original EQ parameter {id} restored as {actual}, expected {expected}",
        );
    }
    assert_eq!(restored.plugin.params().midi_bindings.target(74), Some(104));
    assert_eq!(restored.plugin.params().midi_bindings.target(7), Some(0));
    assert_eq!(
        truce::core::state::snapshot_plugin(&restored.plugin),
        LEGACY_STATE
    );
    let expected: Vec<f32> = LEGACY_AUDIO
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    let actual: Vec<_> = restored.output.iter().flatten().copied().collect();
    assert_eq!(
        actual.len(),
        expected.len(),
        "original capture duration and stereo layout"
    );
    for (index, (&actual, &expected)) in actual.iter().zip(&expected).enumerate() {
        assert!(
            (actual - expected).abs() < 1.0e-6,
            "restored original EQ audio sample {index}: {actual}, expected {expected}",
        );
    }
}
