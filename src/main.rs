#![allow(unused_imports)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate simplelog;

use std::{
    fs::File,
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
    WriteLogger,
};

mod chat;
mod config;
mod connection;
mod mapping;
mod proto;
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
        Region,
        RegionType,
        Vector2,
        Zoner,
    },
};

fn main() {
    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Trace,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Trace,
            Config::default(),
            File::create("latest.log").unwrap(),
        ),
    ])
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

    let mut state = SplinterState::new(
        get_config("./config.ron"),
        Zoner {
            regions: vec![],
            default: 0,
        },
    );
    for (id, addr) in state.config.read().unwrap().server_addresses.iter() {
        state.servers.write().unwrap().insert(
            *id,
            SplinterServer {
                id: *id,
                addr: addr.to_socket_addrs().unwrap().next().unwrap(),
            },
        );
    }
    state::init(&mut state);
    mapping::eid::init(&mut state);
    mapping::uuid::init(&mut state);
    chat::init(&mut state);

    listen_for_clients(Arc::new(state));
}
