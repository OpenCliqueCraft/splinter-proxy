use mcproto_rs::{
    protocol::PacketDirection,
    uuid::UUID4,
    v1_17_0::{
        EntityMetadataFieldData,
        Packet755,
        Packet755Kind,
        PlayerInfoActionList,
    },
};

use super::RelayPass;
use crate::{
    client::SplinterClient,
    mapping::SplinterMapping,
};

inventory::submit! {
    RelayPass(Box::new(|_proxy, connection, client, sender, lazy_packet, destination| {
        if has_uuids(lazy_packet.kind()) {
            if let Ok(ref mut packet) = lazy_packet.packet() {
                let mut map = smol::block_on(connection.map.lock());
                if let Some(_server_id) = map_uuid(&*client, &mut *map, packet, sender) {
                    // *destination = PacketDestination::Server(server_id);
                    *destination = None; // do something here?
                }
            }
        }
    }))
}

pub fn has_uuids(kind: Packet755Kind) -> bool {
    matches!(
        kind,
        Packet755Kind::PlaySpawnEntity
            | Packet755Kind::PlaySpawnLivingEntity
            | Packet755Kind::PlaySpawnPainting
            | Packet755Kind::PlaySpawnPlayer
            | Packet755Kind::PlayBossBar
            | Packet755Kind::PlayServerChatMessage
            | Packet755Kind::PlayEntityMetadata
            | Packet755Kind::PlayPlayerInfo
            | Packet755Kind::PlayEntityProperties
            | Packet755Kind::PlaySpectate
    )
}

pub fn map_uuid(
    client: &SplinterClient,
    map: &mut SplinterMapping,
    packet: &mut Packet755,
    sender: &PacketDirection,
) -> Option<u64> {
    match sender {
        PacketDirection::ClientBound => {
            let server = &client.active_server.load().server;
            let uuid: Option<&mut UUID4> = match packet {
                Packet755::PlaySpawnEntity(body) => Some(&mut body.object_uuid),
                Packet755::PlaySpawnLivingEntity(body) => Some(&mut body.entity_uuid),
                Packet755::PlaySpawnPainting(body) => Some(&mut body.entity_uuid),
                Packet755::PlaySpawnPlayer(body) => Some(&mut body.uuid),
                Packet755::PlayBossBar(body) => Some(&mut body.uuid),
                Packet755::PlayServerChatMessage(body) => Some(&mut body.sender),
                Packet755::PlayEntityMetadata(body) => {
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
                                if let Some(EntityMetadataFieldData::OptUUID(Some(ref mut uuid))) = body.metadata.get_mut(18) {
                                    Some(uuid)
                                }
                                else {
                                    None
                                }
                            }
                            28 => { // fox
                                for index in [19, 20] {
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
                Packet755::PlayPlayerInfo(body) => {
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
                        *uuid = map.map_uuid_server_to_proxy(server.id, *uuid);
                    }
                    None
                }
                Packet755::PlayEntityProperties(body) => {
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
        PacketDirection::ServerBound => {
            if let Packet755::PlaySpectate(body) = packet {
                if let Ok((server_id, server_uuid)) = map.map_uuid_proxy_to_server(body.target) {
                    body.target = server_uuid;
                    return Some(server_id);
                }
            }
        }
    }
    None
}
