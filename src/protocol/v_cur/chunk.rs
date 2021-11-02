use super::{
    PacketDestination,
    RelayPass,
};
use crate::current::{
    PacketLatest,
    PacketLatestKind,
};

inventory::submit! {
    RelayPass(Box::new(|_proxy, connection, client, _sender, lazy_packet, destination| {
        if matches!(lazy_packet.kind(),
            PacketLatestKind::PlayChunkData
            | PacketLatestKind::PlayUpdateLight
            | PacketLatestKind::PlayUnloadChunk
        ) {
            if let Ok(packet) = lazy_packet.packet() {
                let pass_through = smol::block_on(async {
                    match packet {
                        PacketLatest::PlayChunkData(body) => {
                            let chunk = (body.x, body.z);
                            connection.update_chunk(&*client, true, chunk).await
                        },
                        PacketLatest::PlayUpdateLight(body) => {
                            let chunk = (*body.chunk.x, *body.chunk.z);
                            connection.update_chunk(&*client, false, chunk).await
                        },
                        PacketLatest::PlayUnloadChunk(body) => {
                            let chunk = (body.position.x, body.position.z);
                            connection.remove_chunk(&*client, chunk).await
                        },
                        _ => unreachable!(),
                    }
                });
                if !pass_through {
                    *destination = PacketDestination::None;
                }
                /*else {
                    debug!("active through {:?}", lazy_packet.kind());
                }*/
            }
        }
    }))
}
