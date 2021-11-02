use super::{
    PacketDestination,
    RelayPass,
};
use crate::{
    client::SplinterClient,
    current::{
        proto::{
            EntityMetadataFieldData,
            PlayerInfoActionList,
        },
        protocol::PacketDirection,
        uuid::UUID4,
        PacketLatest,
        PacketLatestKind,
    },
    mapping::{
        SplinterMapping,
        SplinterMappingResult,
    },
};

inventory::submit! {
    RelayPass(Box::new(|proxy, _connection, client, sender, lazy_packet, destination| {
        if has_uuids(lazy_packet.kind()) {
            if let Ok(ref mut packet) = lazy_packet.packet() {
                let mut map = smol::block_on(proxy.mapping.lock());
                match map_uuid(&*client, &mut *map, packet, sender) {
                    SplinterMappingResult::Server(server_id) => {
                        *destination = PacketDestination::Server(server_id);
                    }
                    SplinterMappingResult::None => {
                        *destination = PacketDestination::None;
                        debug!("refusing to send packet of type {:?} (no uuid match)", packet);
                    }
                    _ => {}
                }
            }
        }
    }))
}

pub fn has_uuids(kind: PacketLatestKind) -> bool {
    matches!(
        kind,
        PacketLatestKind::PlaySpawnEntity
            | PacketLatestKind::PlaySpawnLivingEntity
            | PacketLatestKind::PlaySpawnPainting
            | PacketLatestKind::PlaySpawnPlayer
            | PacketLatestKind::PlayBossBar
            | PacketLatestKind::PlayServerChatMessage
            | PacketLatestKind::PlayEntityMetadata
            | PacketLatestKind::PlayPlayerInfo
            | PacketLatestKind::PlayEntityProperties
            | PacketLatestKind::PlaySpectate
    )
}

pub fn map_uuid(
    client: &SplinterClient,
    map: &mut SplinterMapping,
    packet: &mut PacketLatest,
    sender: &PacketDirection,
) -> SplinterMappingResult {
    match sender {
        PacketDirection::ClientBound => {
            let server = &client.active_server.load().server;
            if let Some(uuid) = match packet {
                PacketLatest::PlaySpawnEntity(body) => Some(&mut body.object_uuid),
                PacketLatest::PlaySpawnLivingEntity(body) => Some(&mut body.entity_uuid),
                PacketLatest::PlaySpawnPainting(body) => Some(&mut body.entity_uuid),
                PacketLatest::PlaySpawnPlayer(body) => Some(&mut body.uuid),
                _ => None,
            } {
                *uuid = map.register_uuid_mapping(server.id, *uuid);
            } else if let Some(uuid) = match packet {
                PacketLatest::PlayBossBar(body) => Some(&mut body.uuid),
                PacketLatest::PlayServerChatMessage(body) => {
                    if body.sender.to_u128() == 0 {
                        None
                    } else {
                        Some(&mut body.sender)
                    }
                }
                PacketLatest::PlayEntityMetadata(body) => {
                    let proxy_eid = body.entity_id;
                    if let Some(data) = map.entity_data.get(&proxy_eid) {
                        match data.entity_type {
                            37 //horse
                                | 108 // zombie horse
                                | 79 // skeleton horse
                                | 15 // donkey
                                | 46 // llama
                                | 94 // trader llama
                                | 57 // mule 
                                => {
                                if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(18) {
                                    Some(uuid)
                                }
                                else {
                                    None
                                }
                            }
                            29 => { // fox
                                for index in [19, 20] {
                                    if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(index) {
                                        *uuid = if let Some(mapped_id) = map.uuids.get_by_right(&(server.id, *uuid)) {
                                            *mapped_id
                                        } else {
                                            return SplinterMappingResult::None;
                                        }
                                        // special case since there are multiple uuids to map
                                    }
                                }
                                None
                            }
                            8 // cat
                                | 105 // wolf
                                | 62 // parrot
                                => {
                                if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(18) {
                                    Some(uuid)
                                }
                                else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                PacketLatest::PlayPlayerInfo(body) => {
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
                        PlayerInfoActionList::Remove(ref mut arr) => arr.iter_mut().collect(),
                    };
                    for uuid in uuid_arr.into_iter() {
                        *uuid = if let Some(mapped_id) = map.uuids.get_by_right(&(server.id, *uuid))
                        {
                            *mapped_id
                        } else {
                            return SplinterMappingResult::None;
                        }
                    }
                    None
                }
                PacketLatest::PlayEntityProperties(body) => {
                    for property in body.properties.iter_mut() {
                        for modifier in property.modifiers.iter_mut() {
                            modifier.uuid = if let Some(mapped_id) =
                                map.uuids.get_by_right(&(server.id, modifier.uuid))
                            {
                                *mapped_id
                            } else {
                                map.register_uuid_mapping(server.id, modifier.uuid)
                                // UUIDs are unique to the modifier; we are possibly either initializing the mapping or using an existing mapping
                            }
                        }
                    }
                    None
                }
                _ => unreachable!(),
            } {
                *uuid = if let Some(mapped_id) = map.uuids.get_by_right(&(server.id, *uuid)) {
                    *mapped_id
                } else {
                    return SplinterMappingResult::None;
                }
            }
            return SplinterMappingResult::Client;
        }
        PacketDirection::ServerBound => {
            if let PacketLatest::PlaySpectate(body) = packet {
                if let Some((server_id, server_uuid)) = map.uuids.get_by_left(&body.target) {
                    body.target = *server_uuid;
                    return SplinterMappingResult::Server(*server_id);
                }
            }
        }
    }
    SplinterMappingResult::None
}
