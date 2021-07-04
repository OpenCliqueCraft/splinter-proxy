use craftio_rs::CraftAsyncWriter;
use mcproto_rs::v1_17_0::{
    Packet755,
    Packet755Kind,
    PlayClientKeepAliveSpec,
};

use crate::{
    keepalive::unix_time_millis,
    protocol::{
        v755,
        PacketDestination,
        PacketSender,
    },
};

inventory::submit! {
    v755::RelayPass(Box::new(|_proxy, sender, lazy_packet, _map, destination| {
        match sender {
            PacketSender::Proxy(client) => {
                if lazy_packet.kind() == Packet755Kind::PlayClientKeepAlive { // TODO: may want to do something with the keep alive IDs
                    *smol::block_on(client.last_keep_alive.lock()) = unix_time_millis();
                    *destination = PacketDestination::None;
                }
            }
            PacketSender::Server(server, client) => {
                if lazy_packet.kind() == Packet755Kind::PlayServerKeepAlive {
                    if let Ok(Packet755::PlayServerKeepAlive(body)) = lazy_packet.packet() {
                    // respond to server
                    let servers = smol::block_on(client.servers.lock());
                    let mut server_conn = smol::block_on(servers.get(&server.id).unwrap().lock());
                    if let Err(e) = smol::block_on(server_conn.writer.write_packet_async(Packet755::PlayClientKeepAlive(PlayClientKeepAliveSpec {
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
