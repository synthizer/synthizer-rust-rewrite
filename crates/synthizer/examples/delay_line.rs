//! Example that demonstrates using a delay line to delay audio by 1 second.
//!
//! Usage: cargo run --example delay_line <audio_file>

use std::time::Duration;

use synthizer::*;

fn main() -> Result<()> {
    env_logger::init();

    let mut synth = Synthesizer::new_default_output()?;

    let args = std::env::args().collect::<Vec<_>>();
    let file_path = args
        .get(1)
        .expect("Specify a file path as the first argument");
    let file = std::fs::File::open(file_path).unwrap();

    let (controller, mut media);

    // Create a delay line that can hold 2 seconds of stereo audio
    let delay_line = DelayLineHandle::<[f64; 2]>::new_defaulting(
        std::num::NonZeroUsize::new(synth.duration_to_samples(Duration::from_secs(2))).unwrap(),
    );

    let _handle = {
        // Calculate 1 second delay in samples
        let delay_samples = synth.duration_to_samples(Duration::from_secs(1));

        let mut batch = synth.batch();

        (controller, media) = batch.make_media(file)?;

        let program = Program::new();

        // Create a chain that writes media to the delay line
        let writer = program
            .chain_media::<2>(&mut media, ChannelFormat::Stereo)
            .write_delay_line(&delay_line);
        program.add_fragment(writer)?;

        // Create a chain that reads from the delay line with 1 second delay
        let reader = program
            .new_chain()
            .start_as(delay_samples)
            .read_delay_line(&delay_line)
            .to_audio_device(ChannelFormat::Stereo);
        program.add_fragment(reader)?;

        batch.mount(program)?
    };

    controller.play()?;

    println!("Playing audio with 1 second delay for 30 seconds...");
    std::thread::sleep(Duration::from_secs(30));
    Ok(())
}
