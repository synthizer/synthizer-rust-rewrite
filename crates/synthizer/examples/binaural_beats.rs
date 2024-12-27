use anyhow::Result;
use synthizer::*;

const L_FREQ: f64 = 200.0;
const R_FREQ: f64 = 203.0;

fn main() -> Result<()> {
    let mut synth = Synthesizer::new_default_output().unwrap();

    let pi2 = 2.0f64 * std::f64::consts::PI;

    let left = Chain::new(L_FREQ)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0f64)
        .inline_mul(Chain::new(pi2))
        .sin()
        .boxed();
    let right = Chain::new(R_FREQ)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0)
        .inline_mul(Chain::new(pi2))
        .sin()
        .boxed();

    let ready = left.join(right).discard_and_default::<()>();

    let to_dev = ready.to_audio_device(ChannelFormat::Stereo);

    let _handle = {
        let mut batch = synth.batch();
        batch.mount(to_dev)?
    };

    std::thread::sleep(std::time::Duration::from_secs(5));

    Ok(())
}
