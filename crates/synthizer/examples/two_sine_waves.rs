use synthizer::*;

fn main() {
    let pi2 = 2.0f64 * std::f64::consts::PI;
    let chain1 = Chain::new(500f64)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0f64)
        .inline_mul(Chain::new(pi2))
        .sin();
    let chain2 = Chain::new(600f64)
        .divide_by_sr()
        .periodic_sum(1.0f64, 0.0)
        .inline_mul(Chain::new(pi2))
        .sin();
    let added = chain1 + chain2;
    let ready = added * Chain::new(0.1f64);
    let to_dev = ready.to_audio_device();

    let mut synth = Synthesizer::new_default_output().unwrap();
    let _handle = {
        let mut batch = synth.batch();
        batch.mount(to_dev)
    };

    std::thread::sleep(std::time::Duration::from_secs(5));
}
