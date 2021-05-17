use std::{
    self,
    sync::{
        Arc,
        RwLock,
    },
};

use crate::config::SplinterProxyConfiguration;

pub struct SplinterState {
    pub config: RwLock<SplinterProxyConfiguration>,
    pub player_count: RwLock<u32>,
    pub id: RwLock<i32>,
}

pub struct SplinterClient {
    pub name: String,
}

impl SplinterState {
    pub fn new(config: SplinterProxyConfiguration) -> Arc<SplinterState> {
        Arc::new(SplinterState {
            config: RwLock::new(config),
            player_count: RwLock::new(0),
            id: RwLock::new(0),
        })
    }
}
