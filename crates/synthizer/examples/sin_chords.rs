use anyhow::Result;
use synthizer::*;

const C_FREQ: f64 = 261.63;
const E_FREQ: f64 = 329.63;
const G_FREQ: f64 = 392.00;

fn main() -> Result<()> {
    let mut synth = Synthesizer::new_default_output().unwrap();

    let pi2 = 2.0f64 * std::f64::consts::PI;

    let freq1;
    let freq2;
    let freq3;

    {
        let mut b = synth.batch();
        freq1 = b.allocate_slot::<f64>(C_FREQ);
        freq2 = b.allocate_slot::<f64>(E_FREQ);
        freq3 = b.allocate_slot::<f64>(G_FREQ);
    }

    let program = Program::new();

    let note1 = program
        .new_chain()
        .read_slot(&freq1, C_FREQ)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0f64)
        .inline_mul(program.new_chain().start_as(pi2))
        .sin();
    let note2 = program
        .new_chain()
        .read_slot(&freq2, E_FREQ)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0)
        .inline_mul(program.new_chain().start_as(pi2))
        .sin();
    let note3 = program
        .new_chain()
        .read_slot(&freq3, G_FREQ)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0)
        .inline_mul(program.new_chain().start_as(pi2))
        .sin();

    let added = note1 + note2 + note3;
    let ready = added * program.new_chain().start_as(0.1f64);
    let to_dev = ready.to_audio_device(ChannelFormat::Mono);

    to_dev.mount()?;

    let handle = {
        let mut batch = synth.batch();
        batch.mount(program)?
    };

    std::thread::sleep(std::time::Duration::from_secs(1));

    {
        let mut batch = synth.batch();
        batch.replace_slot_value(&handle, &freq1, C_FREQ * 2.0)?;
        batch.replace_slot_value(&handle, &freq2, E_FREQ * 2.0)?;
        batch.replace_slot_value(&handle, &freq3, G_FREQ * 2.0)?;
    }

    std::thread::sleep(std::time::Duration::from_secs(1));

    Ok(())
}
