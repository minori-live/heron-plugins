//! Run only against the archived original Heron 0.6.5 source, as an example.
//! This is a fixture capture harness, not a current-code fixture regenerator.

use std::{path::PathBuf, time::Duration};

use heron_eq::{EqParams, Plugin};
use truce::prelude::*;
use truce_test::{InputSource, driver};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(std::env::args().nth(1).ok_or("expected output directory")?);
    std::fs::create_dir_all(&output)?;
    let mut plugin = Plugin::create();
    for (id, value) in [
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
        assert!(
            plugin.params().get_plain(id).is_some(),
            "original parameter {id}"
        );
        plugin.params().set_plain(id, value);
    }
    plugin.params().midi_bindings.bind(74, 104);
    plugin.params().midi_bindings.bind(7, 0);
    // Capture a canonical state emitted by the original implementation. Some
    // float ranges canonicalize when normalized values are restored.
    let mut state = truce::core::state::snapshot_plugin(&plugin);
    let mut canonical = false;
    for _ in 0..8 {
        truce::core::state::restore_plugin(&mut plugin, &state)?;
        let next = truce::core::state::snapshot_plugin(&plugin);
        if next == state {
            canonical = true;
            break;
        }
        state = next;
    }
    assert!(
        canonical,
        "original state must reach its stable serialized form"
    );
    std::fs::write(output.join("heron-0.6.5-nondefault.pluginstate"), &state)?;
    let captured = driver!(Plugin)
        .sample_rate(48000.0)
        .duration(Duration::from_millis(20))
        .state_blob(state)
        .input(InputSource::Generator(Box::new(|frame, sample_rate| {
            let t = frame as f64 / sample_rate;
            (0.1 * (std::f64::consts::TAU * 440.0 * t).sin()
                + 0.05 * (std::f64::consts::TAU * 8000.0 * t).sin()) as f32
        })))
        .run();
    let audio: Vec<_> = captured
        .output
        .iter()
        .flat_map(|channel| channel.iter().flat_map(|sample| sample.to_le_bytes()))
        .collect();
    std::fs::write(output.join("heron-0.6.5-nondefault.f32le"), audio)?;
    std::fs::write(
        output.join("heron-0.6.5-parameter-ids.json"),
        serde_json::to_string_pretty(
            &Plugin::param_infos_static()
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
        )?,
    )?;
    // Preserve Heron's declared defaults, units and host automation mapping,
    // without recording upstream implementation details or Debug output.
    let contract: Vec<_> = Plugin::param_infos_static()
        .iter()
        .map(|p| {
            let quarter_points = [0.25, 0.5, 0.75].map(|v| p.range.denormalize(v));
            serde_json::json!({
                "id": p.id,
                "defaultPlain": p.default_plain,
                "unit": p.unit.as_str(),
                "minimum": p.range.min(),
                "maximum": p.range.max(),
                "steps": p.range.step_count().map(std::num::NonZeroU32::get),
                "flags": p.flags.bits(),
                "quarterPoints": quarter_points,
            })
        })
        .collect();
    std::fs::write(
        output.join("heron-0.6.5-default-parameters.json"),
        serde_json::to_string_pretty(&contract)?,
    )?;
    let fit: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/fit-preset.json"))?;
    let config = serde_json::from_value(fit["config"].clone())?;
    let defaults = EqParams::default();
    let changes: Vec<_> = defaults
        .normalized_values(&config)
        .into_iter()
        .filter(|(id, value)| (defaults.get_normalized(*id).unwrap() - value).abs() > 1.0e-12)
        .collect();
    std::fs::write(
        output.join("fit-automation.json"),
        serde_json::to_string_pretty(&changes)?,
    )?;
    println!(
        "captured original EQ state and {} audio channels",
        captured.output.len()
    );
    Ok(())
}
