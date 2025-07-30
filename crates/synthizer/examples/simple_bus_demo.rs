//! Simple demonstration of the bus system concept.
//!
//! This example shows how buses work conceptually, even though
//! the full bus signal infrastructure isn't complete yet.

use std::sync::Arc;
use std::time::Duration;

use synthizer::*;

fn main() -> Result<()> {
    println!("Creating synthesizer...");
    let mut synth = Synthesizer::new_default_output()?;

    // Start a batch for all our operations
    let mut batch = synth.batch();

    // Create a stereo bus for mixing
    let mix_bus: Arc<Bus<[f64; 2]>> = batch.create_bus();
    println!("Created stereo mix bus with ID: {:?}", mix_bus.id());

    // Create a simple program that would write to the bus
    let mut writer_program = Program::new();
    let _writer_bus_link = writer_program.link_output_bus(&mix_bus);
    println!("Created writer program that outputs to the bus");

    // Create another program that would read from the bus
    let mut reader_program = Program::new();
    let _reader_bus_link = reader_program.link_input_bus(&mix_bus);
    println!("Created reader program that inputs from the bus");

    // Mount the programs
    let _writer_handle = batch.mount(writer_program)?;
    let _reader_handle = batch.mount(reader_program)?;
    
    println!("\nPrograms mounted. The dependency system will ensure:");
    println!("- Writer program executes before reader program");
    println!("- Bus acts as the communication channel between them");
    
    // Drop the batch to send all commands
    drop(batch);
    
    println!("\nRunning for 2 seconds to demonstrate the system is active...");
    std::thread::sleep(Duration::from_secs(2));
    
    println!("Done!");
    Ok(())
}