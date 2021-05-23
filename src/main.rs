#![allow(unused_imports)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate simplelog;

use std::{
    net::ToSocketAddrs,
    sync::Arc,
};

use simplelog::{
    ColorChoice,
    CombinedLogger,
    Config,
    LevelFilter,
    TermLogger,
    TerminalMode,
};

mod chat;
mod config;
mod connection;
mod mapping;
mod state;
mod zoning;
use crate::{
    config::get_config,
    connection::listen_for_clients,
    state::{
        SplinterServer,
        SplinterState,
    },
    zoning::{
        BasicZoner,
        SquareRegion,
        Vector2,
        Zoner,
    },
};

fn main() {
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Trace,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )])
    .expect("Logger failed to initialize");
    info!("Starting Splinter proxy");

    // Zoner test code, implements same structure as splinter-prototype
    // Server 0 is inner 200x200 centered at spawn, Server 1 is everything
    // outside of that box.

    // let mut zoner = BasicZoner::new(
    //     vec![(
    //         0, // Server ID for inner box
    //         SquareRegion::new(
    //             Vector2 {
    //                 x: -100,
    //                 z: -100,
    //             },
    //             Vector2 {
    //                 x: 100,
    //                 z: 100,
    //             },
    //         ),
    //     )],
    //     1, // Fallback server ID
    // );

    // println!(
    //     "zone check: {:?}",
    //     zoner.get_zone(&Vector2 {
    //         x: 101,
    //         z: 100
    //     })
    // );

    // map.insert(
    //     PacketLatestKind::PlayBlockChange,
    //     Box::new(|state: Arc<SplinterState>, raw_packet: RawPacketLatest| {
    //         info!("blockupdate");

    //         let packet = match raw_packet.deserialize() {
    //             Ok(packet) => packet,
    //             Err(e) => {
    //                 error!("Failed to deserialize packet: {}", e);
    //                 return MapAction::None;
    //             }
    //         };

    //         {
    //             let mut d = state.id.write().unwrap();
    //             *d += 1;
    //         }

    //         if let PacketLatest::PlayBlockChange(mut data) = packet {
    //             data.block_id = (*state.id.read().unwrap()).into();
    //             MapAction::Client(PacketLatest::PlayBlockChange(data))
    //         } else {
    //             MapAction::Client(packet)
    //         }
    //     }),
    // );

    let mut state = SplinterState::new(get_config("./config.ron"));
    // single server specific, temporary
    let server_id = state.next_server_id();
    state.servers.write().unwrap().insert(
        server_id,
        SplinterServer {
            id: server_id,
            addr: state
                .config
                .read()
                .unwrap()
                .server_address
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap(),
        },
    );
    chat::init(&mut state);

    listen_for_clients(Arc::new(state));
}
