//! This example demonstrates how to use Synthizer to write to wave files rather than an audio device.
//!
//! Usage: `cargo run --example write_wave_file -- output_path`
use anyhow::Result;

use synthizer as syz;

fn main() -> Result<()> {
    env_logger::init();

    let args = std::env::args().collect::<Vec<_>>();
    let file_path = args
        .get(1)
        .expect("Specify a file path as the first argument");

    let server = syz::Server::new_inline()?;
    let audio_output = syz::nodes::AudioOutputNode::new(&server, syz::ChannelFormat::Stereo)?;
    // Middle c. Note that using a whole frequency means that one second won't click at the end, so even though middle C
    // is technically 261.63, we use 261.
    let sine = syz::nodes::TrigWaveformNode::new_sin(&server, 261f64)?;
    server.connect(&sine, 0, &audio_output, 0)?;

    // A buffer of stereo data.
    let mut buffer = vec![0.0f32; syz::SR as usize * 2];

    server.synthesize_stereo(&mut buffer[..])?;

    // Hound is an easy library to write wave files.  More complex examples (e.g. to lossy files) should consider other
    // crates such as symphonia.
    let spec = hound::WavSpec {
        channels: 2,
        // These are the settings for 32-bit float, what we get from Synthizer.
        sample_format: hound::SampleFormat::Float,
        bits_per_sample: 32,
        sample_rate: syz::SR,
    };
    let mut writer = hound::WavWriter::create(file_path, spec)?;

    for s in buffer {
        writer.write_sample(s)?;
    }

    // Catch any errors from hound.
    writer.finalize()?;

    Ok(())
}
