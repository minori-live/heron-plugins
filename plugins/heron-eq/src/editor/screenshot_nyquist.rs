//! Comparable native evidence for live and frozen spectra at the Nyquist boundary.

use super::*;
use heron_plugin_dsp::eq::{EqBand, EqConfig};

#[test]
#[ignore = "manual native Nyquist render evidence"]
fn render_nyquist_native_editors() {
    let directory = std::env::var_os("HERON_EQ_NYQUIST_EVIDENCE_DIRECTORY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("../../out/eq-nyquist"));
    std::fs::create_dir_all(&directory).expect("Nyquist evidence directory");
    layout_check::reset();

    let (low_params, _low_state) = processed_fixture(32_000.0);
    let (high_params, _high_state) = processed_fixture(96_000.0);
    let (frozen_params, _frozen_state) = processed_fixture(96_000.0);
    render_scene::<false>(&directory, "live-32khz", low_params);
    render_scene::<true>(&directory, "frozen-96khz-at-32khz", frozen_params);
    render_scene::<false>(&directory, "live-96khz", high_params);

    let manifest: Vec<_> = [
        ("live-32khz", 32_000, 32_000),
        ("frozen-96khz-at-32khz", 32_000, 96_000),
        ("live-96khz", 96_000, 96_000),
    ]
    .into_iter()
    .flat_map(|(name, sample_rate, spectrum_sample_rate)| {
        [(1120, 760, ""), (1040, 700, "-minimum")].map(|(width, height, suffix)| {
            serde_json::json!({
                "file": format!("{name}{suffix}.png"),
                "logical_viewport": [width, height],
                "sample_rate": sample_rate,
                "spectrum_sample_rate": spectrum_sample_rate,
                "visible": ["Pre", "Post", "Ext", "EQ", "collision shading"],
                "source": "native EqUi with actual processor-fed telemetry",
            })
        })
    })
    .collect();
    std::fs::write(
        directory.join("scenes.json"),
        serde_json::to_vec_pretty(&manifest).expect("Nyquist scene manifest JSON"),
    )
    .expect("Nyquist scene manifest");
}

fn processed_fixture(sample_rate: f64) -> (Arc<EqParams>, crate::EqDspState) {
    let config = EqConfig {
        bands: vec![
            EqBand {
                id: 1,
                shape: EqShape::LowCut,
                frequency_hz: 70.0,
                ..Default::default()
            },
            EqBand {
                id: 2,
                frequency_hz: 1300.0,
                gain_db: 4.0,
                ..Default::default()
            },
            EqBand {
                id: 3,
                shape: EqShape::HighShelf,
                frequency_hz: 7500.0,
                gain_db: 8.0,
                ..Default::default()
            },
            EqBand {
                id: 4,
                shape: EqShape::HighCut,
                frequency_hz: 13_000.0,
                slope_db_oct: 24.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let params = Arc::new(EqParams::default());
    for (id, value) in params.normalized_values(&config) {
        params.set_normalized(id, value);
    }
    let mut state = crate::EqDspState::default();
    state.reset(&params, sample_rate);
    process_samples(&mut state, &params, sample_rate);
    (params, state)
}

fn process_samples(state: &mut crate::EqDspState, params: &EqParams, sample_rate: f64) {
    let mut random = 123_456_789_u32;
    let main: Vec<_> = (0..32768)
        .map(|index| {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let noise = (random as f32 / u32::MAX as f32 - 0.5) * 0.16;
            let phase = std::f32::consts::TAU * index as f32 / sample_rate as f32;
            noise + 0.045 * (phase * 1300.0).sin() + 0.035 * (phase * 14_500.0).sin()
        })
        .collect();
    let external: Vec<_> = main.iter().map(|sample| sample * 1.8).collect();
    let mut left = vec![0.0; main.len()];
    let mut right = vec![0.0; main.len()];
    let inputs = [
        main.as_slice(),
        main.as_slice(),
        external.as_slice(),
        external.as_slice(),
    ];
    let mut outputs = [left.as_mut_slice(), right.as_mut_slice()];
    let mut buffer = AudioBuffer::from_slices_checked(&inputs, &mut outputs, main.len());
    state.process(params, &mut buffer, &EventList::with_capacity(0));
}

struct NyquistScene<const FROZEN: bool> {
    ui: EqUi,
    _reset_state: Option<crate::EqDspState>,
}

impl<const FROZEN: bool> IcedPlugin<EqParams> for NyquistScene<FROZEN> {
    type Message = EqMessage;

    fn new(params: Arc<EqParams>) -> Self {
        let mut ui = EqUi::new(params);
        ui.external = true;
        ui.collision = true;
        let reset_state = if FROZEN {
            // The host can lower its sample rate after the user freezes the analyzer.
            // Reset the same instance and retain its UI and original processor-fed bins.
            ui.frozen = Some(ui.params.telemetry.spectrum());
            ui.frozen_display = Some(ui.params.telemetry.display_spectrum());
            let mut state = crate::EqDspState::default();
            state.reset(&ui.params, 32_000.0);
            process_samples(&mut state, &ui.params, 32_000.0);
            Some(state)
        } else {
            None
        };
        Self {
            ui,
            _reset_state: reset_state,
        }
    }

    fn update(
        &mut self,
        message: Message<EqMessage>,
        params: &ParamCache<EqParams>,
        context: &PluginContext<EqParams>,
    ) -> Task<Message<EqMessage>> {
        self.ui.update(message, params, context)
    }

    fn view<'a>(&'a self, params: &'a ParamCache<EqParams>) -> Element<'a, Message<EqMessage>> {
        layout_check::checked(self.ui.view(params))
    }

    fn theme(&self) -> truce_iced::iced::Theme {
        self.ui.theme()
    }
}

fn render_scene<const FROZEN: bool>(
    directory: &std::path::Path,
    name: &str,
    params: Arc<EqParams>,
) {
    for (size, suffix) in [((1120, 760), ""), ((1040, 700), "-minimum")] {
        let _source_state = FROZEN.then(|| {
            let mut state = crate::EqDspState::default();
            state.reset(&params, 96_000.0);
            process_samples(&mut state, &params, 96_000.0);
            state
        });
        let mut editor = IcedEditor::<EqParams, NyquistScene<FROZEN>>::new(params.clone(), size);
        let erased: Arc<dyn Params> = params.clone();
        let (pixels, width, height) = editor
            .screenshot(erased)
            .expect("native Nyquist screenshot");
        assert!(
            width > 0
                && height > 0
                && u64::from(width) * u64::from(size.1) == u64::from(height) * u64::from(size.0),
            "native evidence dimensions must preserve the logical viewport aspect ratio"
        );
        truce::core::screenshot::save_png(
            &directory.join(format!("{name}{suffix}.png")),
            &pixels,
            width,
            height,
        );
    }
}
