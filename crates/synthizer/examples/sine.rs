use synthizer as syz;

fn main() -> syz::Result<()> {
    env_logger::init();
    let server = synthizer::ServerHandle::new_default_device()?;
    // middle c
    server.start_sin(256.0)?;

    std::thread::sleep(std::time::Duration::from_secs(30));
    Ok(())
}
