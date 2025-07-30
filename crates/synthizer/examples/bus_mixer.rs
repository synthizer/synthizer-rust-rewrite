//! Example that demonstrates mixing multiple audio files through a bus.
//!
//! Usage: cargo run --example bus_mixer <path1> <path2> ...
//!
//! This creates one program for each audio file that writes to a shared bus,
//! and another program that reads from the bus and outputs to the audio device.

use std::env;
use std::time::Duration;

use synthizer::*;

fn main() -> Result<()> {
    // Get command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <audio_file1> [audio_file2] ...", args[0]);
        std::process::exit(1);
    }

    let paths: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();

    println!("Creating synthesizer...");
    let mut synth = Synthesizer::new_default_output()?;

    // Start a batch for all our operations
    let mut batch = synth.batch();

    // Create a stereo bus for mixing all audio streams
    let mix_bus = batch.create_bus::<[f64; 2]>();
    println!("Created mix bus");

    // Create programs for each audio file
    let mut handles = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        println!("Loading audio file {}: {}", i + 1, path);

        // Open the file and create media
        let file = std::fs::File::open(path)?;
        let (controller, mut media) = batch.make_media(file)?;

        // Create a program that plays this media and writes to the bus
        let mut program = Program::new();

        // Create the signal chain: media -> add to bus
        let chain = media.start_chain::<2>(ChannelFormat::Stereo);
        let chain = program.link_output_bus(&mix_bus).frame_add(chain);

        program.add_fragment(chain)?;

        // Mount the program
        let handle = batch.mount(program)?;
        handles.push((handle, controller));

        println!("Created program for file {}", i + 1);
    }

    // Create the output program that reads from the bus and outputs to speakers
    let mut output_program = Program::new();

    // Create signal chain: read from bus -> output to speakers
    let output_chain = output_program
        .link_input_bus(&mix_bus)
        .read()
        .to_audio_device(ChannelFormat::Stereo);

    output_program.add_fragment(output_chain)?;

    // Mount the output program
    let _output_handle = batch.mount(output_program)?;
    println!("Created output program");

    // Drop the batch to send all commands
    drop(batch);

    // Start playback on all media
    println!(
        "\nStarting playback of {} files mixed together...",
        paths.len()
    );
    for (i, (_, controller)) in handles.iter_mut().enumerate() {
        controller.play()?;
        println!("Started playback of file {}", i + 1);
    }

    // Play for 30 seconds or until all files finish
    println!("\nPlaying for 30 seconds. Press Ctrl+C to stop.");
    std::thread::sleep(Duration::from_secs(30));

    println!("\nStopping...");
    Ok(())
}
