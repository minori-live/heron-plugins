use super::*;

struct Identity;
impl Renderer for Identity {
    fn render_channels(
        &mut self,
        input: &[Vec<f32>],
        _shift: f32,
    ) -> Result<Vec<Vec<f32>>, DspError> {
        assert_eq!(input.len(), 2, "the stereo window must arrive as one batch");
        assert!(input.iter().all(|channel| channel.len() == MODEL_WINDOW));
        Ok(input.to_vec())
    }
}

struct ShiftGain;
impl Renderer for ShiftGain {
    fn render_channels(
        &mut self,
        input: &[Vec<f32>],
        shift: f32,
    ) -> Result<Vec<Vec<f32>>, DspError> {
        Ok(input
            .iter()
            .map(|channel| {
                channel
                    .iter()
                    .map(|sample| sample * (1.0 + shift))
                    .collect()
            })
            .collect())
    }
}

fn geometry() -> Geometry {
    // Small windows isolate adapter indexing from the neural model's context size.
    Geometry {
        context: 32,
        hop: 128,
        overlap: 32,
        latency: 256,
    }
}

fn packet(start: u64, channels: usize, shift: f32, signal: impl Fn(u64) -> [f32; 2]) -> Packet {
    let mut packet = Packet::new(start, channels, shift);
    packet.len = crate::worker::PACKET_FRAMES;
    for (offset, sample) in packet.samples.iter_mut().enumerate() {
        *sample = signal(start + offset as u64);
    }
    packet
}

#[test]
fn one_stereo_batch_keeps_channels_crop_and_overlap_at_original_sample_times() {
    let mut stream = Stream::new(Identity, MODEL_RATE, geometry());
    let signal = |sample: u64| {
        [
            (sample as f32 * 0.031).sin() * 0.4,
            (sample as f32 * 0.047).cos() * -0.25,
        ]
    };
    stream.push(&packet(0, 2, 0.0, signal));
    assert!(stream.render_ready().unwrap().is_none());
    stream.push(&packet(128, 2, 0.0, signal));
    let first = stream.render_ready().unwrap().unwrap();
    stream.push(&packet(256, 2, 0.0, signal));
    let second = stream.render_ready().unwrap().unwrap();
    assert_eq!(first.start, 0);
    assert_eq!(second.start, 128);
    for rendered in [first, second] {
        assert_eq!(rendered.samples.len(), 128);
        for (offset, actual) in rendered.samples.iter().enumerate() {
            let expected = signal(rendered.start + offset as u64);
            for channel in 0..2 {
                assert!(
                    (actual[channel] - expected[channel]).abs() < 6e-8,
                    "sample {}, channel {channel}: {} != {}",
                    rendered.start + offset as u64,
                    actual[channel],
                    expected[channel]
                );
            }
        }
    }
}

#[test]
fn missing_returned_channel_fails_before_publishing_partial_stereo_audio() {
    struct MissingRight;
    impl Renderer for MissingRight {
        fn render_channels(
            &mut self,
            input: &[Vec<f32>],
            _: f32,
        ) -> Result<Vec<Vec<f32>>, DspError> {
            Ok(vec![input[0].clone()])
        }
    }
    let mut stream = Stream::new(MissingRight, MODEL_RATE, geometry());
    stream.push(&packet(0, 2, 0.0, |_| [0.2, -0.7]));
    stream.push(&packet(128, 2, 0.0, |_| [0.2, -0.7]));
    assert!(matches!(
        stream.render_ready(),
        Err(DspError::OutputShape {
            model: "Revox",
            output: "waveform batch",
        })
    ));
}

#[test]
fn pitch_automation_uses_the_output_window_start_and_crossfades_its_boundary() {
    let mut stream = Stream::new(ShiftGain, MODEL_RATE, geometry());
    let signal = |_| [0.2, -0.1];
    stream.push(&packet(0, 2, 0.0, signal));
    stream.push(&packet(128, 2, 0.5, signal));
    let first = stream.render_ready().unwrap().unwrap();
    assert!(first.samples.iter().all(|sample| *sample == [0.2, -0.1]));
    stream.push(&packet(256, 2, 1.0, signal));
    let second = stream.render_ready().unwrap().unwrap();
    assert_eq!(second.start, 128);
    // The latest received packet is already +1, but this output interval starts
    // at input sample128 and must retain its +0.5 setting.
    assert!(
        second.samples[32..]
            .iter()
            .all(|sample| *sample == [0.3, -0.15])
    );
    assert!(
        second.samples[..32]
            .windows(2)
            .all(|pair| pair[0][0] < pair[1][0])
    );
    assert!(second.samples[0][0] > 0.2 && second.samples[31][0] < 0.3);
    stream.push(&packet(384, 2, 1.0, signal));
    let third = stream.render_ready().unwrap().unwrap();
    assert_eq!(third.start, 256);
    assert!(
        third.samples[32..]
            .iter()
            .all(|sample| *sample == [0.4, -0.2])
    );
}

#[test]
fn a_layout_boundary_starts_a_new_region_without_previous_tail_or_controls() {
    let mut stream = Stream::new(ShiftGain, MODEL_RATE, geometry());
    stream.push(&packet(0, 2, 1.0, |_| [0.2, -0.1]));
    stream.push(&packet(128, 2, 1.0, |_| [0.2, -0.1]));
    let previous = stream.render_ready().unwrap().unwrap();
    assert_eq!(previous.samples[0], [0.4, -0.2]);

    stream.push(&packet(1000, 1, 0.0, |_| [-0.3, 0.0]));
    assert!(stream.render_ready().unwrap().is_none());
    stream.push(&packet(1128, 1, 0.0, |_| [-0.3, 0.0]));
    let restarted = stream.render_ready().unwrap().unwrap();
    assert_eq!(restarted.start, 1000);
    assert!(
        restarted
            .samples
            .iter()
            .all(|sample| *sample == [-0.3, 0.0])
    );
}

#[test]
fn new_stream_and_layout_region_discard_renderer_history_from_warmup_and_old_audio() {
    struct HistoryGain(f32);
    impl Renderer for HistoryGain {
        fn reset_stream(&mut self) {
            self.0 = 2.0;
        }

        fn render_channels(
            &mut self,
            input: &[Vec<f32>],
            _: f32,
        ) -> Result<Vec<Vec<f32>>, DspError> {
            let gain = self.0;
            self.0 += 1.0;
            Ok(input
                .iter()
                .map(|channel| channel.iter().map(|sample| sample * gain).collect())
                .collect())
        }
    }

    let mut stream = Stream::new(HistoryGain(99.0), MODEL_RATE, geometry());
    for start in [0, 128] {
        stream.push(&packet(start, 2, 0.0, |_| [0.2, -0.1]));
    }
    let first = stream.render_ready().unwrap().unwrap();
    assert!(first.samples.iter().all(|sample| *sample == [0.4, -0.2]));

    // A new mono region must use a fresh excitation history and no old tail.
    for start in [1_000, 1_128] {
        stream.push(&packet(start, 1, 0.0, |_| [-0.3, 0.0]));
    }
    let restarted = stream.render_ready().unwrap().unwrap();
    assert_eq!(restarted.start, 1_000);
    assert!(
        restarted
            .samples
            .iter()
            .all(|sample| *sample == [-0.6, 0.0])
    );
}
