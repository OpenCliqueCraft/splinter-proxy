use std::fs::File;

use simplelog::{
    ColorChoice,
    CombinedLogger,
    Config,
    LevelFilter,
    TermLogger,
    TerminalMode,
    WriteLogger,
};

pub const LATEST_LOG_FILENAME: &'static str = "./latest.log";

pub fn init() -> anyhow::Result<()> {
    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Debug,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Debug, // setting to trace will result in a lot from the async stuff
            Config::default(),
            File::create(LATEST_LOG_FILENAME).unwrap(),
        ),
    ])?;
    Ok(())
}
