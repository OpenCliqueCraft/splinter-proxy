use std::sync::Arc;

use craftio_rs::CraftSyncWriter;
use mcproto_rs::{
    types::VarInt,
    v1_16_3::EntityMetadataFieldData,
};

use crate::{
    proto::{
        PacketLatest,
        PacketLatestKind,
        RawPacketLatest,
    },
    state::{
        EntityData,
        SplinterState,
    },
};

/// Maps a server-side entity id to a proxy-side entity id
///
/// Will create a new mapping between entity ids if no map exists
pub fn map_eid(state: &SplinterState, server_id: u64, server_eid: i32) -> i32 {
    let table = state.eid_table.read().unwrap();
    if let Some(entry) = table.get_by_right(&(server_id, server_eid.into())) {
        return *entry;
    }
    let proxy_eid: i32 = state.eid_gen.lock().unwrap().take_id() as i32;
    drop(table);
    let mut table = state.eid_table.write().unwrap();
    table.insert(proxy_eid, (server_id, server_eid));
    proxy_eid
}

/// Maps a proxy-side entity id to a server-side entity id.
///
/// None if failed to find existing mapping between EIDs
pub fn map_serverbound_eid(state: &SplinterState, proxy_eid: i32) -> Option<(u64, i32)> {
    state
        .eid_table
        .read()
        .unwrap()
        .get_by_left(&proxy_eid)
        .map(|val| *val)
}

pub fn init(state: &mut SplinterState) {
    for kind in [
        // probably not as fast as writing code individually for each packet type, but im not writing all that out. maybe a macro in the future

        // clientbound:
        PacketLatestKind::PlayEntityAnimation,
        PacketLatestKind::PlayBlockBreakAnimation,
        PacketLatestKind::PlayEntityStatus,
        PacketLatestKind::PlayOpenHorseWindow,
        PacketLatestKind::PlayEntityPosition,
        PacketLatestKind::PlayEntityPositionAndRotation,
        PacketLatestKind::PlayEntityRotation,
        PacketLatestKind::PlayEntityMovement,
        PacketLatestKind::PlayRemoveEntityEffect,
        PacketLatestKind::PlayEntityHeadLook,
        PacketLatestKind::PlayCamera,
        PacketLatestKind::PlayEntityVelocity,
        PacketLatestKind::PlayEntityEquipment,
        PacketLatestKind::PlayEntitySoundEffect,
        PacketLatestKind::PlayEntityTeleport,
        PacketLatestKind::PlayEntityProperties,
        PacketLatestKind::PlayEntityEffect,
    ] {
        state.server_packet_map.add_action(
            kind,
            Box::new(|_client, server, state, lazy_packet| {
                enum IdRef<'a> {
                    VI(&'a mut VarInt),
                    I32(&'a mut i32),
                }
                let eid: IdRef = match match lazy_packet.packet() {
                    Ok(packet) => packet,
                    Err(_) => return false,
                } {
                    PacketLatest::PlayEntityAnimation(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayBlockBreakAnimation(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityStatus(body) => IdRef::I32(&mut body.entity_id),
                    PacketLatest::PlayOpenHorseWindow(body) => IdRef::I32(&mut body.entity_id),
                    PacketLatest::PlayEntityPosition(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityPositionAndRotation(body) => {
                        IdRef::VI(&mut body.entity_id)
                    }
                    PacketLatest::PlayEntityRotation(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityMovement(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayRemoveEntityEffect(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityHeadLook(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayCamera(body) => IdRef::VI(&mut body.camera_id),
                    PacketLatest::PlayEntityVelocity(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityEquipment(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntitySoundEffect(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityTeleport(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityProperties(body) => IdRef::VI(&mut body.entity_id),
                    PacketLatest::PlayEntityEffect(body) => IdRef::VI(&mut body.entity_id),
                    _ => unreachable!(),
                };
                match eid {
                    IdRef::VI(eid) => *eid = map_eid(state, server.id, (*eid).into()).into(),
                    IdRef::I32(eid) => *eid = map_eid(state, server.id, *eid),
                }
                true
            }),
        );
    }
    for kind in [
        PacketLatestKind::PlaySpawnEntity,
        PacketLatestKind::PlaySpawnExperienceOrb,
        PacketLatestKind::PlaySpawnLivingEntity,
        PacketLatestKind::PlaySpawnPainting,
        PacketLatestKind::PlaySpawnPlayer,
    ] {
        state.server_packet_map.add_action(
            kind,
            Box::new(|_client, server, state, lazy_packet| {
                let (eid, entity_type): (&mut VarInt, i32) = match match lazy_packet.packet() {
                    Ok(packet) => packet,
                    Err(_) => return false,
                } {
                    PacketLatest::PlaySpawnEntity(body) => {
                        let etype = body.entity_type.into();
                        match etype {
                            107 => {
                                body.data = map_eid(state, server.id, body.data).into(); // fishing bobber
                            }
                            2 // arrow
                                | 79 // spectral arrow
                                | 39 // fireball
                                | 76 // small fireball
                                | 15 // dragon fireball
                                | 99 // wither skull
                                => {
                                if body.data > 0 {
                                    body.data = map_eid(state, server.id, body.data - 1) + 1;
                                }
                            },
                            _ => {},
                        }
                        (&mut body.entity_id, etype)
                    },
                    PacketLatest::PlaySpawnExperienceOrb(body) => (&mut body.entity_id, 24), // https://wiki.vg/Entity_metadata
                    PacketLatest::PlaySpawnLivingEntity(body) => (&mut body.entity_id, body.entity_type.into()),
                    PacketLatest::PlaySpawnPainting(body) => (&mut body.entity_id, 55),
                    PacketLatest::PlaySpawnPlayer(body) => (&mut body.entity_id, 106),
                    _ => unreachable!(),
                };
                let proxy_eid = map_eid(state, server.id, (*eid).into());
                *eid = proxy_eid.into();
                state.eid_data.write().unwrap().insert(proxy_eid, EntityData { id: proxy_eid, entity_type: entity_type });
                true
            }),
        );
    }
    state.server_packet_map.add_action(
        PacketLatestKind::PlayFacePlayer,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayFacePlayer(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                if let Some(mut target) = body.entity.as_mut() {
                    target.entity_id = map_eid(state, server.id, target.entity_id.into()).into();
                }
            }
            true
        }),
    );
    state.server_packet_map.add_action(
        PacketLatestKind::PlayAttachEntity,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayAttachEntity(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                body.attached_entity_id = map_eid(state, server.id, body.attached_entity_id);
                body.holding_entity_id = map_eid(state, server.id, body.holding_entity_id);
            }
            true
        }),
    );
    state.server_packet_map.add_action(
        PacketLatestKind::PlaySetPassengers,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlaySetPassengers(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                body.entity_id = map_eid(state, server.id, body.entity_id.into()).into();
                for passenger in body.passenger_entitiy_ids.iter_mut() {
                    *passenger = map_eid(state, server.id, (*passenger).into()).into();
                }
            }
            true
        }),
    );
    state.server_packet_map.add_action(
        PacketLatestKind::PlayCollectItem,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayCollectItem(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                body.collected_entity_id =
                    map_eid(state, server.id, body.collected_entity_id.into()).into();
                body.collector_entity_id =
                    map_eid(state, server.id, body.collector_entity_id.into()).into();
            }
            true
        }),
    );
    state.server_packet_map.add_action(
        PacketLatestKind::PlayEntityMetadata,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayEntityMetadata(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                let proxy_eid = map_eid(state, server.id, body.entity_id.into());
                body.entity_id = proxy_eid.into();
                if let Some(data) = state.eid_data.read().unwrap().get(&proxy_eid) {
                    match data.entity_type {
                        27 => {
                            // firework id
                            if let Some(data) = body.metadata.get_mut(8) {
                                if let EntityMetadataFieldData::OptVarInt(ref mut id) = data {
                                    let found_id: i32 = (*id).into();
                                    if found_id > 0 {
                                        *id = (map_eid(state, server.id, found_id - 1) + 1).into();
                                    }
                                }
                            }
                        }
                        107 => {
                            // fishing hook
                            if let Some(data) = body.metadata.get_mut(7) {
                                if let EntityMetadataFieldData::VarInt(ref mut id) = data {
                                    let found_id: i32 = (*id).into();
                                    if found_id > 0 {
                                        *id = (map_eid(state, server.id, found_id - 1) + 1).into();
                                    }
                                }
                            }
                        }
                        97 => {
                            // wither
                            for index in [15, 16, 17] {
                                if let Some(data) = body.metadata.get_mut(index) {
                                    if let EntityMetadataFieldData::VarInt(ref mut id) = data {
                                        let found_id: i32 = (*id).into();
                                        if found_id > 0 {
                                            *id = (map_eid(state, server.id, found_id - 1) + 1) // docs dont say + 1, but Im assuming that is the case here
                                                .into();
                                        }
                                    }
                                }
                            }
                        }
                        31 | 17 => {
                            // guardian or elder guardian
                            if let Some(data) = body.metadata.get_mut(16) {
                                if let EntityMetadataFieldData::VarInt(ref mut id) = data {
                                    let found_id: i32 = (*id).into();
                                    if found_id > 0 {
                                        *id = (map_eid(state, server.id, found_id - 1) + 1).into();
                                        // same as above comment
                                    }
                                }
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
        PacketLatestKind::PlayDestroyEntities,
        Box::new(|_client, server, state, lazy_packet| {
            if let PacketLatest::PlayDestroyEntities(body) = match lazy_packet.packet() {
                Ok(packet) => packet,
                Err(_) => return false,
            } {
                for eid in body.entity_ids.iter_mut() {
                    let server_eid: i32 = (*eid).into();
                    *eid = map_eid(state, server.id, (*eid).into()).into();
                    if let Some((proxy_eid, _)) = state
                        .eid_table
                        .write()
                        .unwrap()
                        .remove_by_right(&(server.id, server_eid))
                    {
                        state.eid_gen.lock().unwrap().return_id(proxy_eid as u64);
                    }
                }
            }
            true
        }),
    );
    for kind in [
        // serverbound:
        PacketLatestKind::PlayQueryEntityNbt,
        PacketLatestKind::PlayInteractEntity,
        PacketLatestKind::PlayEntityAction,
        PacketLatestKind::PlayUpdateCommandBlockMinecart,
    ] {
        state.server_packet_map.add_action(
            kind,
            Box::new(|client, _server, state, lazy_packet| {
                if let Ok(packet) = lazy_packet.packet() {
                    let eid: &mut VarInt = match packet {
                        PacketLatest::PlayQueryEntityNbt(body) => &mut body.entity_id,
                        PacketLatest::PlayInteractEntity(body) => &mut body.entity_id,
                        PacketLatest::PlayEntityAction(body) => &mut body.entity_id,
                        PacketLatest::PlayUpdateCommandBlockMinecart(body) => &mut body.entity_id,
                        _ => unreachable!(),
                    };
                    if let Some((server_id, server_eid)) = map_serverbound_eid(state, (*eid).into())
                    {
                        *eid = server_eid.into();
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
                false
            }),
        );
    }
}
