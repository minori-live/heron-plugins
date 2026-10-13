use super::*;

struct Identity;
impl Renderer for Identity {
    fn render(&mut self, input: &[f32], _shift: f32) -> Result<Vec<f32>, DspError> {
        Ok(input.to_vec())
    }
}

struct ShiftGain;
impl Renderer for ShiftGain {
    fn render(&mut self, input: &[f32], shift: f32) -> Result<Vec<f32>, DspError> {
        Ok(input.iter().map(|sample| sample * (1.0 + shift)).collect())
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
fn context_crop_and_overlap_keep_stereo_identity_at_original_sample_times() {
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
fn a_missing_packet_starts_a_new_region_without_previous_tail_or_controls() {
    let mut stream = Stream::new(ShiftGain, MODEL_RATE, geometry());
    stream.push(&packet(0, 2, 1.0, |_| [0.2, -0.1]));
    stream.push(&packet(128, 2, 1.0, |_| [0.2, -0.1]));
    let previous = stream.render_ready().unwrap().unwrap();
    assert_eq!(previous.samples[0], [0.4, -0.2]);

    stream.push(&packet(1000, 2, 0.0, |_| [-0.3, 0.7]));
    assert!(stream.render_ready().unwrap().is_none());
    stream.push(&packet(1128, 2, 0.0, |_| [-0.3, 0.7]));
    let restarted = stream.render_ready().unwrap().unwrap();
    assert_eq!(restarted.start, 1000);
    assert!(
        restarted
            .samples
            .iter()
            .all(|sample| *sample == [-0.3, 0.7])
    );
}
