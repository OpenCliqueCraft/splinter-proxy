use craftio_rs::CraftAsyncWriter;
use mcproto_rs::v1_16_3::{
    Packet753,
    Packet753Kind,
    PlayClientKeepAliveSpec,
};

use crate::{
    keepalive::unix_time_millis,
    protocol::{
        v753,
        PacketDestination,
        PacketSender,
    },
};

inventory::submit! {
    v753::RelayPass(Box::new(|_proxy, sender, lazy_packet, _map, destination| {
        match sender {
            PacketSender::Proxy(client) => {
                if lazy_packet.kind() == Packet753Kind::PlayClientKeepAlive { // TODO: may want to do something with the keep alive IDs
                    *smol::block_on(client.last_keep_alive.lock()) = unix_time_millis();
                    *destination = PacketDestination::None;
                }
            }
            PacketSender::Server(server, client) => {
                if lazy_packet.kind() == Packet753Kind::PlayServerKeepAlive {
                    if let Ok(Packet753::PlayServerKeepAlive(body)) = lazy_packet.packet() {
                    // respond to server
                    let servers = smol::block_on(client.servers.lock());
                    let mut server_conn = smol::block_on(servers.get(&server.id).unwrap().lock());
                    if let Err(e) = smol::block_on(server_conn.writer.write_packet_async(Packet753::PlayClientKeepAlive(PlayClientKeepAliveSpec {
                        id: body.id,
                    }))) {
                        error!("Failed to send keep alive from \"{}\" to server id {}: {}", &client.name, server.id, e);
                    }
                }
                }
            }
        }
    }))
}
