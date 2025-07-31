use anyhow::Result;
use synthizer::*;

const L_FREQ: f64 = 200.0;
const R_FREQ: f64 = 203.0;

fn main() -> Result<()> {
    let mut synth = Synthesizer::new_default_output().unwrap();

    let pi2 = 2.0f64 * std::f64::consts::PI;

    let program = Program::new();

    let left = program
        .new_chain()
        .start_as(L_FREQ)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0f64)
        .inline_mul(program.new_chain().start_as(pi2))
        .sin();
    let right = program
        .new_chain()
        .start_as(R_FREQ)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0)
        .inline_mul(program.new_chain().start_as(pi2))
        .sin();

    let ready = left.join(right).discard_and_default::<()>();

    let to_dev = ready.to_audio_device(ChannelFormat::Stereo);

    to_dev.mount()?;

    let _handle = {
        let mut batch = synth.batch();
        batch.mount(program)?
    };

    std::thread::sleep(std::time::Duration::from_secs(5));

    Ok(())
}
