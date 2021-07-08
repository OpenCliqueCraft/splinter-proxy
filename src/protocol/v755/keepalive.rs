use craftio_rs::CraftAsyncWriter;
use mcproto_rs::{
    protocol::PacketDirection,
    v1_17_0::{
        Packet755,
        Packet755Kind,
        PlayClientKeepAliveSpec,
    },
};

use crate::{
    keepalive::unix_time_millis,
    protocol::v755,
};

inventory::submit! {
    v755::RelayPass(Box::new(|_proxy, _connection, client, direction, lazy_packet, destination| {
        match direction {
            PacketDirection::ServerBound => {
                if lazy_packet.kind() == Packet755Kind::PlayClientKeepAlive { // TODO: may want to do something with the keep alive IDs
                    *smol::block_on(client.last_keep_alive.lock()) = unix_time_millis();
                    *destination = None;
                }
            }
            PacketDirection::ClientBound => {
                if lazy_packet.kind() == Packet755Kind::PlayServerKeepAlive {
                    if let Ok(Packet755::PlayServerKeepAlive(body)) = lazy_packet.packet() {
                    // respond to server

                    let server_conn = client.active_server.load();
                    if let Err(e) = smol::block_on(async { server_conn.writer.lock().await.write_packet_async(Packet755::PlayClientKeepAlive(PlayClientKeepAliveSpec {
                        id: body.id,
                    })).await }) {
                        error!("Failed to send keep alive from \"{}\" to server id {}: {}", &client.name, server_conn.server.id, e);
                    }
                }
                }
            }
        }
    }))
}
