use synthizer::Result;

pub fn main() -> Result<()> {
    println!(
        "Default output device: {}",
        synthizer::get_default_output_device()?
    );
    println!("Output devices:");

    for d in synthizer::audio_device::get_all_output_devices()? {
        println!("    {}", d);
    }

    Ok(())
}
