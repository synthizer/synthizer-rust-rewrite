use synthizer::*;

fn main() -> Result<()> {
    env_logger::init();

    let mut synth = Synthesizer::new_default_output()?;

    let args = std::env::args().collect::<Vec<_>>();
    let file_path = args
        .get(1)
        .expect("Specify a file path as the first argument");
    let file = std::fs::File::open(file_path).unwrap();

    let wavetable_handle;
    let phase_slot;
    let rate_slot;
    let playing_slot;
    
    let handle = {
        let mut batch = synth.batch();

        // Build wavetable from file
        wavetable_handle = WaveTable::builder(file)
            .target_sample_rate(SR as u32)
            .build_with_batch(&mut batch)?;

        // Create control slots
        phase_slot = batch.allocate_slot(0.0f64);
        rate_slot = batch.allocate_slot(1.0f64);
        playing_slot = batch.allocate_slot(0.0f64); // 0.0 = paused, 1.0 = playing

        // For now, use a simple fixed increment (1.0 = normal speed)
        let increment = 1.0;
        
        // Create the wavetable reading signal with linear interpolation and looping
        let output_signal = Chain::new(wavetable_handle.read_linear::<[f64; 2], true>(increment))
            .to_audio_device(ChannelFormat::Stereo);

        batch.mount(output_signal)?
    };

    println!(
        "
Wavetable Player Commands:

play      - Start playback
pause     - Stop playback  
rate <x>  - Set playback rate (1.0 = normal, 2.0 = double speed, etc.)
pos <x>   - Set position in frames
info      - Show wavetable info
quit      - Exit

Note: This example uses linear interpolation with looping enabled.
To test different interpolation modes, modify the example code.
"
    );

    loop {
        use std::io::Write;

        print!("Command> ");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        let mut line = line.trim().split(' ');

        let cmd = line.next();
        let Some(cmd) = cmd else {
            continue;
        };

        match cmd {
            "quit" => break,
            "play" => {
                let mut batch = synth.batch();
                batch.replace_slot_value(&handle, &playing_slot, 1.0)?;
                println!("Playing");
            }
            "pause" => {
                let mut batch = synth.batch();
                batch.replace_slot_value(&handle, &playing_slot, 0.0)?;
                println!("Paused");
            }
            "rate" => {
                let Some(rate_str) = line.next() else {
                    println!("Missing rate parameter");
                    continue;
                };

                let Ok(rate) = rate_str.parse::<f64>() else {
                    println!("Invalid rate: {rate_str}");
                    continue;
                };

                let mut batch = synth.batch();
                batch.replace_slot_value(&handle, &rate_slot, rate)?;
                println!("Playback rate set to {rate}");
            }
            "pos" => {
                let Some(pos_str) = line.next() else {
                    println!("Missing position parameter");
                    continue;
                };

                let Ok(pos) = pos_str.parse::<f64>() else {
                    println!("Invalid position: {pos_str}");
                    continue;
                };

                let mut batch = synth.batch();
                batch.replace_slot_value(&handle, &phase_slot, pos)?;
                println!("Position set to {pos} frames");
            }
            "info" => {
                println!("Wavetable loaded with {} frames", wavetable_handle.frame_count());
                println!("Sample rate: {} Hz", wavetable_handle.sample_rate());
                println!("Channels: {}", wavetable_handle.channel_count());
            }
            _ => {
                println!("Unknown command: {cmd}");
            }
        }
    }

    Ok(())
}