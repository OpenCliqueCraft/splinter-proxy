use mcproto_rs::{
    uuid::UUID4,
    v1_16_3::{
        EntityMetadataFieldData,
        Packet753,
        Packet753Kind,
        PlayerInfoActionList,
        RawPacket753,
    },
};

use super::RelayPassFn;
use crate::{
    mapping::SplinterMapping,
    protocol::{
        PacketDestination,
        PacketSender,
    },
};

inventory::submit! {
    RelayPassFn(Box::new(|proxy, sender, mut lazy_packet, map, destination| {
        if has_uuids(lazy_packet.kind()) {
            if let Ok(ref mut packet) = lazy_packet.packet() {
                if let Some(server_id) = map_uuid(map, packet, sender) {
                    *destination = PacketDestination::Server(server_id);
                }
            }
        }
    }))
}

pub fn has_uuids(kind: Packet753Kind) -> bool {
    matches!(
        kind,
        Packet753Kind::PlaySpawnEntity
            | Packet753Kind::PlaySpawnLivingEntity
            | Packet753Kind::PlaySpawnPainting
            | Packet753Kind::PlaySpawnPlayer
            | Packet753Kind::PlayBossBar
            | Packet753Kind::PlayServerChatMessage
            | Packet753Kind::PlayEntityMetadata
            | Packet753Kind::PlayPlayerInfo
            | Packet753Kind::PlayEntityProperties
            | Packet753Kind::PlaySpectate
    )
}

pub fn map_uuid(
    map: &mut SplinterMapping,
    packet: &mut Packet753,
    sender: &PacketSender,
) -> Option<u64> {
    match sender {
        PacketSender::Server(server) => {
            let uuid: Option<&mut UUID4> = match packet {
                Packet753::PlaySpawnEntity(body) => Some(&mut body.object_uuid),
                Packet753::PlaySpawnLivingEntity(body) => Some(&mut body.entity_uuid),
                Packet753::PlaySpawnPainting(body) => Some(&mut body.entity_uuid),
                Packet753::PlaySpawnPlayer(body) => Some(&mut body.uuid),
                Packet753::PlayBossBar(body) => Some(&mut body.uuid),
                Packet753::PlayServerChatMessage(body) => Some(&mut body.sender),
                Packet753::PlayEntityMetadata(body) => {
                    let proxy_eid = body.entity_id;
                    if let Some(data) = map.entity_data.get(&proxy_eid) {
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
                                    Some(uuid)
                                }
                                else {
                                    None
                                }
                            }
                            28 => { // fox
                                for index in [18, 19] {
                                    if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(index) {
                                        *uuid = map.map_uuid_server_to_proxy(server.id, *uuid);
                                        // special case since there are multiple uuids to map
                                    }
                                }
                                None
                            }
                            7 // cat
                                | 100 // wolf
                                | 57 // parrot
                                => {
                                if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(17) {
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
                Packet753::PlayPlayerInfo(body) => {
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
                        *uuid = map.map_uuid_server_to_proxy(server.id, *uuid);
                    }
                    None
                }
                Packet753::PlayEntityProperties(body) => {
                    for property in body.properties.iter_mut() {
                        for modifier in property.modifiers.iter_mut() {
                            modifier.uuid = map.map_uuid_server_to_proxy(server.id, modifier.uuid);
                        }
                    }
                    None
                }
                _ => unreachable!(),
            };
            if let Some(uuid) = uuid {
                *uuid = map.map_uuid_server_to_proxy(server.id, *uuid);
            }
        }
        PacketSender::Proxy(_) => {
            if let Packet753::PlaySpectate(body) = packet {
                if let Ok((server_id, server_uuid)) = map.map_uuid_proxy_to_server(body.target) {
                    body.target = server_uuid;
                    return Some(server_id);
                }
            }
        }
    }
    None
}
