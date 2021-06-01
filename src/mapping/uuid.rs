use craftio_rs::CraftSyncWriter;
use mcproto_rs::{
    uuid::UUID4,
    v1_16_3::{
        EntityMetadataFieldData,
        PlayerInfoActionList,
    },
};

use crate::{
    proto::{
        PacketLatest,
        PacketLatestKind,
    },
    state::SplinterState,
};

/// Maps a server-side UUID to a proxy-side UUID
///
/// Will create a new mapping between the two UUIDS if no map exists
pub fn map_uuid(state: &SplinterState, server_id: u64, server_uuid: UUID4) -> UUID4 {
    if let Some(entry) = state
        .uuid_table
        .read()
        .unwrap()
        .get_by_right(&(server_id, server_uuid))
    {
        return *entry;
    }
    let proxy_uuid = UUID4::random();
    state
        .uuid_table
        .write()
        .unwrap()
        .insert(proxy_uuid, (server_id, server_uuid));
    proxy_uuid
}

/// MAps a proxy-side UUID to a server-side UUID
///
/// None if failed to find existing mapping between UUIDs
pub fn map_serverbound_uuid(state: &SplinterState, proxy_uuid: UUID4) -> Option<(u64, UUID4)> {
    state
        .uuid_table
        .read()
        .unwrap()
        .get_by_left(&proxy_uuid)
        .map(|val| *val)
}

pub fn init(state: &mut SplinterState) {
    for kind in [
        PacketLatestKind::PlaySpawnEntity,
        PacketLatestKind::PlaySpawnLivingEntity,
        PacketLatestKind::PlaySpawnPainting,
        PacketLatestKind::PlaySpawnPlayer,
        PacketLatestKind::PlayBossBar,
        PacketLatestKind::PlayServerChatMessage,
    ] {
        state.server_packet_map.add_action(
            kind,
            Box::new(|_client, server, state, lazy_packet| {
                let uuid: &mut UUID4 = match match lazy_packet.packet() {
                    Ok(packet) => packet,
                    Err(_) => return false,
                } {
                    PacketLatest::PlaySpawnEntity(body) => &mut body.object_uuid,
                    PacketLatest::PlaySpawnLivingEntity(body) => &mut body.entity_uuid,
                    PacketLatest::PlaySpawnPainting(body) => &mut body.entity_uuid,
                    PacketLatest::PlaySpawnPlayer(body) => &mut body.uuid,
                    PacketLatest::PlayBossBar(body) => &mut body.uuid,
                    PacketLatest::PlayServerChatMessage(body) => &mut body.sender,
                    _ => unreachable!(),
                };
                *uuid = map_uuid(&*state, server.id, *uuid);
                true
            }),
        );
    }
    state.server_packet_map.add_action(
        PacketLatestKind::PlayEntityMetadata,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayEntityMetadata(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                let proxy_eid = body.entity_id;
                if let Some(data) = state.eid_data.read().unwrap().get(&proxy_eid) {
                    match data.entity_type {
                        33 //horse
                            | 103 // zombie horse
                            | 74 // skeleton horse
                            | 14 // donkey
                            | 42 // llama
                            | 79 // trader llama
                            | 52 // mule 
                            => {
                            if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(17) {
                                *uuid = map_uuid(&*state, server.id, *uuid);
                            }
                        }
                        28 => { // fox
                            for index in [18, 19] {
                                if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(index) {
                                    *uuid = map_uuid(&*state, server.id, *uuid);
                                }
                            }
                        }
                        7 // cat
                            | 100 // wolf
                            | 57 // parrot
                            => {
                            if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(17) {
                                *uuid = map_uuid(&*state, server.id, *uuid);
                            }
                        }
                        _ => {}
                    }
                }
            }
            true
        }),
    );
    state.server_packet_map.add_action(
        PacketLatestKind::PlayPlayerInfo,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayPlayerInfo(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                let uuid_arr: Vec<&mut UUID4> = match body.actions {
                    PlayerInfoActionList::Add(ref mut arr) => {
                        arr.iter_mut().map(|plinfo| &mut plinfo.uuid).collect()
                    }
                    PlayerInfoActionList::UpdateGameMode(ref mut arr) => {
                        arr.iter_mut().map(|plinfo| &mut plinfo.uuid).collect()
                    }
                    PlayerInfoActionList::UpdateLatency(ref mut arr) => {
                        arr.iter_mut().map(|plinfo| &mut plinfo.uuid).collect()
                    }
                    PlayerInfoActionList::UpdateDisplayName(ref mut arr) => {
                        arr.iter_mut().map(|plinfo| &mut plinfo.uuid).collect()
                    }
                    PlayerInfoActionList::Remove(ref mut arr) => {
                        arr.iter_mut().map(|uuid| uuid).collect()
                    }
                };
                for uuid in uuid_arr.into_iter() {
                    *uuid = map_uuid(&*state, server.id, *uuid);
                }
            }
            true
        }),
    );
    state.server_packet_map.add_action(
        PacketLatestKind::PlayEntityProperties,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayEntityProperties(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                for property in body.properties.iter_mut() {
                    for modifier in property.modifiers.iter_mut() {
                        modifier.uuid = map_uuid(&*state, server.id, modifier.uuid);
                    }
                }
            }
            true
        }),
    );
    state.server_packet_map.add_action(
        // WARNING. this does not overlap with the eid's version, but this is an area where it could overlap given any packet kinds overlap, so take caution
        PacketLatestKind::PlaySpectate,
        Box::new(|client, _server, state, lazy_packet| {
            if let Ok(packet) = lazy_packet.packet() {
                if let PacketLatest::PlaySpectate(body) = packet {
                    if let Some((server_id, server_uuid)) =
                        map_serverbound_uuid(&*state, body.target)
                    {
                        body.target = server_uuid;
                        if let Err(e) = client
                            .servers
                            .read()
                            .unwrap()
                            .get(&server_id)
                            .unwrap()
                            .writer
                            .lock()
                            .unwrap()
                            .write_packet(packet.clone())
                        {
                            error!("Failed to write packet to server {}: {}", server_id, e);
                        }
                    }
                }
            }
            false
        }),
    );
}
