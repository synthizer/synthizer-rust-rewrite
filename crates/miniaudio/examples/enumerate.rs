use synthizer_miniaudio::*;

pub fn main() -> Result<()> {
    env_logger::init();
    let devices = enumerate_output_devices()?;

    println!("Output devices:");
    for d in devices {
        let platform_default = if d.is_platform_default() {
            "(platform default)"
        } else {
            ""
        };
        println!("    {} {}", d.name(), platform_default);
    }

    Ok(())
}
