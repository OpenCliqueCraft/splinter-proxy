use craftio_rs::CraftAsyncWriter;
use mcproto_rs::{
    protocol::PacketDirection,
    v1_16_3::{
        Packet753,
        Packet753Kind,
        PlayClientKeepAliveSpec,
    },
};

use crate::{
    keepalive::unix_time_millis,
    protocol::v753,
};

inventory::submit! {
    v753::RelayPass(Box::new(|_proxy, _connection, client, direction, lazy_packet, destination| {
        match direction {
            PacketDirection::ServerBound => {
                if lazy_packet.kind() == Packet753Kind::PlayClientKeepAlive { // TODO: may want to do something with the keep alive IDs
                    *smol::block_on(client.last_keep_alive.lock()) = unix_time_millis();
                    *destination = None;
                }
            }
            PacketDirection::ClientBound => {
                if lazy_packet.kind() == Packet753Kind::PlayServerKeepAlive {
                    if let Ok(Packet753::PlayServerKeepAlive(body)) = lazy_packet.packet() {
                        // respond to server
                        let server_conn = client.active_server.load();
                        if let Err(e) = smol::block_on(async { server_conn.writer.lock().await.write_packet_async(Packet753::PlayClientKeepAlive(PlayClientKeepAliveSpec {
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
