use std::fs::File;

use simplelog::{
    ColorChoice,
    CombinedLogger,
    ConfigBuilder,
    LevelFilter,
    TermLogger,
    TerminalMode,
    WriteLogger,
};

pub const LATEST_LOG_FILENAME: &str = "./latest.log";

pub fn init() -> anyhow::Result<()> {
    let config = ConfigBuilder::default().set_time_to_local(true).build();
    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Debug,
            config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Debug, /* setting to trace will result in a lot from the async libraries used in this project */
            config,
            File::create(LATEST_LOG_FILENAME).unwrap(),
        ),
    ])?;
    Ok(())
}
