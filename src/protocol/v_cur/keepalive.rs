use craftio_rs::CraftAsyncWriter;

use crate::{
    current::{
        proto::{
            Packet756 as PacketLatest,
            Packet756Kind as PacketLatestKind,
            PlayClientKeepAliveSpec,
        },
        protocol::PacketDirection,
    },
    keepalive::unix_time_millis,
    protocol::v_cur,
};

inventory::submit! {
    v_cur::RelayPass(Box::new(|_proxy, _connection, client, direction, lazy_packet, destination| {
        match direction {
            PacketDirection::ServerBound => {
                if lazy_packet.kind() == PacketLatestKind::PlayClientKeepAlive { // TODO: may want to do something with the keep alive IDs
                    *smol::block_on(client.last_keep_alive.lock()) = unix_time_millis();
                    *destination = None;
                }
            }
            PacketDirection::ClientBound => {
                if lazy_packet.kind() == PacketLatestKind::PlayServerKeepAlive {
                    if let Ok(PacketLatest::PlayServerKeepAlive(body)) = lazy_packet.packet() {
                        // respond to server

                        let server_conn = client.active_server.load();
                        if let Err(e) = smol::block_on(async { server_conn.writer.lock().await.write_packet_async(PacketLatest::PlayClientKeepAlive(PlayClientKeepAliveSpec {
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
