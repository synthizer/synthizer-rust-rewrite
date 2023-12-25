use std::thread::sleep;
use std::time::Duration;

use synthizer as syz;

const SR: f64 = 44100.0;
const DUR: usize = 44100;
const FL: f64 = 300.0f64;
const FR: f64 = 302.0f64;

fn synthesize_binaural_beats() -> Vec<f32> {
    let mut ret = vec![];
    for i in 0..DUR {
        for f in [FL, FR] {
            ret.push((2.0f64 * std::f64::consts::PI * f * (i as f64) / SR).sin() as f32);
        }
    }

    ret
}
fn main() -> syz::Result<()> {
    env_logger::init();

    let server = syz::Server::new_default_device()?;
    let audio_output = syz::nodes::AudioOutputNode::new(&server, syz::ChannelFormat::Stereo)?;

    let source = syz::sample_sources::VecSource::builder()
        .set_sample_rate((SR as u64).try_into().unwrap())
        .set_channel_format(syz::ChannelFormat::Stereo)
        .build_with_data(synthesize_binaural_beats())
        .unwrap();
    let source_node = syz::nodes::SampleSourcePlayerNode::new(&server, source)?;

    server.connect(&source_node, 0, &audio_output, 0)?;

    sleep(Duration::from_secs(2));

    Ok(())
}
