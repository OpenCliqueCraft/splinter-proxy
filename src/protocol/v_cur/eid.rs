use super::RelayPass;
use crate::{
    client::SplinterClient,
    current::{
        proto::{
            EntityMetadataFieldData,
            Packet756 as PacketLatest,
            Packet756Kind as PacketLatestKind,
        },
        protocol::PacketDirection,
        types::VarInt,
    },
    mapping::{
        EntityData,
        SplinterMapping,
    },
};

inventory::submit! {
    RelayPass(Box::new(|_proxy, connection, client, sender, lazy_packet, destination| {
        if has_eids(lazy_packet.kind()) {
            if let Ok(ref mut packet) = lazy_packet.packet() {
                let mut map = smol::block_on(connection.map.lock());
                if let Some(_server_id) = map_eid(&*client, &mut map, packet, sender) {
                    // *destination = PacketDestination::Server(server_id);
                    *destination = None; // do something here?
                }
            }
        }
    }))
}

pub fn has_eids(kind: PacketLatestKind) -> bool {
    matches!(
        kind,
        PacketLatestKind::PlayEntityAnimation
            | PacketLatestKind::PlayBlockBreakAnimation
            | PacketLatestKind::PlayEntityStatus
            | PacketLatestKind::PlayOpenHorseWindow
            | PacketLatestKind::PlayEntityPosition
            | PacketLatestKind::PlayEntityPositionAndRotation
            | PacketLatestKind::PlayEntityRotation
            | PacketLatestKind::PlayRemoveEntityEffect
            | PacketLatestKind::PlayEntityHeadLook
            | PacketLatestKind::PlayCamera
            | PacketLatestKind::PlayEntityVelocity
            | PacketLatestKind::PlayEntityEquipment
            | PacketLatestKind::PlayEntitySoundEffect
            | PacketLatestKind::PlayEntityTeleport
            | PacketLatestKind::PlayEntityProperties
            | PacketLatestKind::PlayEntityEffect
            | PacketLatestKind::PlayFacePlayer
            | PacketLatestKind::PlayAttachEntity
            | PacketLatestKind::PlayEndCombatEvent
            | PacketLatestKind::PlayDeathCombatEvent
            | PacketLatestKind::PlaySpawnEntity
            | PacketLatestKind::PlaySpawnExperienceOrb
            | PacketLatestKind::PlaySpawnLivingEntity
            | PacketLatestKind::PlaySpawnPainting
            | PacketLatestKind::PlaySpawnPlayer
            | PacketLatestKind::PlaySetPassengers
            | PacketLatestKind::PlayCollectItem
            | PacketLatestKind::PlayEntityMetadata
            | PacketLatestKind::PlayDestroyEntity
            | PacketLatestKind::PlayQueryEntityNbt
            | PacketLatestKind::PlayInteractEntity
            | PacketLatestKind::PlayEntityAction
            | PacketLatestKind::PlayUpdateCommandBlockMinecart
    )
}
pub fn map_eid(
    client: &SplinterClient,
    map: &mut SplinterMapping,
    packet: &mut PacketLatest,
    sender: &PacketDirection,
) -> Option<u64> {
    match sender {
        PacketDirection::ClientBound => {
            let server = &client.active_server.load().server;
            let mut entity_data: Option<EntityData> = None;
            let (nums, varnums): (Vec<&mut i32>, Vec<&mut VarInt>) = match packet {
                // TODO: is it possible to use something less intensive than a vec here?
                // trivial
                PacketLatest::PlayEntityAnimation(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayBlockBreakAnimation(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntityStatus(body) => (vec![&mut body.entity_id], vec![]),
                PacketLatest::PlayOpenHorseWindow(body) => (vec![&mut body.entity_id], vec![]),
                PacketLatest::PlayEntityPosition(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntityPositionAndRotation(body) => {
                    (vec![], vec![&mut body.entity_id])
                }
                PacketLatest::PlayEntityRotation(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayRemoveEntityEffect(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntityHeadLook(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayCamera(body) => (vec![], vec![&mut body.camera_id]),
                PacketLatest::PlayEntityVelocity(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntityEquipment(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntitySoundEffect(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntityTeleport(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntityProperties(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEntityEffect(body) => (vec![], vec![&mut body.entity_id]),
                PacketLatest::PlayEndCombatEvent(body) => (vec![&mut body.entity_id], vec![]),
                PacketLatest::PlayDeathCombatEvent(body) => (vec![&mut body.entity_id], vec![]),

                // slightly more complex
                PacketLatest::PlayFacePlayer(body) => {
                    if let Some(target) = body.entity.as_mut() {
                        (vec![], vec![&mut target.entity_id])
                    } else {
                        (vec![], vec![])
                    }
                }
                PacketLatest::PlayAttachEntity(body) => (
                    if body.holding_entity_id < 0 {
                        vec![&mut body.attached_entity_id]
                    } else {
                        vec![&mut body.attached_entity_id, &mut body.holding_entity_id]
                    },
                    vec![],
                ),
                PacketLatest::PlayCollectItem(body) => (
                    vec![],
                    vec![&mut body.collected_entity_id, &mut body.collector_entity_id],
                ),
                PacketLatest::PlaySetPassengers(body) => {
                    // TODO: spelling error in mcproto
                    (
                        vec![],
                        body.passenger_entitiy_ids.iter_mut().fold(
                            vec![&mut body.entity_id],
                            |mut acc, item| {
                                acc.push(item);
                                acc
                            },
                        ),
                    )
                }

                // entity spawning
                PacketLatest::PlaySpawnEntity(body) => {
                    let entity_type = *body.entity_type;
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type,
                    });
                    // debug!("entity spawn type: {}", entity_type);
                    (
                        match entity_type {
                            107 => {
                                // bobber
                                vec![&mut body.data]
                            }
                            2 | 79 | 39 | 76 | 15 | 99 => {
                                // arrow, spectral arrow, fireball, small fireball, dragon fireball, wither skull
                                if body.data > 0 {
                                    // body.data is option varint. we need to specially handle this
                                    body.data =
                                        map.map_eid_server_to_proxy(server.id, body.data - 1) + 1;
                                }
                                vec![]
                            }
                            _ => {
                                // debug!("got type entity spawn type without eid {}", entity_type);
                                vec![]
                            }
                        },
                        vec![&mut body.entity_id],
                    )
                }
                PacketLatest::PlaySpawnExperienceOrb(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 24,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                PacketLatest::PlaySpawnLivingEntity(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: *body.entity_type,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                PacketLatest::PlaySpawnPainting(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 55,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                PacketLatest::PlaySpawnPlayer(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 106,
                    });
                    (vec![], vec![&mut body.entity_id])
                }
                // complex
                PacketLatest::PlayEntityMetadata(body) => {
                    // we specially need to handle mapping here for the proxy side eid
                    let proxy_eid = map.map_eid_server_to_proxy(server.id, *body.entity_id);
                    body.entity_id = proxy_eid.into();
                    if let Some(data) = map.entity_data.get(&proxy_eid) {
                        match data.entity_type {
                            27 => {
                                // fireworks
                                if let Some(EntityMetadataFieldData::OptVarInt(ref mut id)) =
                                    body.metadata.get_mut(9)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        *id = (map
                                            .map_eid_server_to_proxy(server.id, found_id - 1)
                                            + 1)
                                        .into();
                                    }
                                }
                            }
                            107 => {
                                // fishing hook
                                if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                    body.metadata.get_mut(8)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        *id = (map
                                            .map_eid_server_to_proxy(server.id, found_id - 1)
                                            + 1)
                                        .into();
                                    }
                                }
                            }
                            97 => {
                                // wither
                                for index in [16, 17, 18] {
                                    if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                        body.metadata.get_mut(index)
                                    {
                                        let found_id: i32 = **id;
                                        if found_id > 0 {
                                            *id = (map
                                                .map_eid_server_to_proxy(server.id, found_id - 1)
                                                + 1) // docs dont say + 1, but Im assuming that is the case here
                                            .into();
                                        }
                                    }
                                }
                            }
                            31 | 17 => {
                                // guardian or elder guardian
                                if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                    body.metadata.get_mut(17)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        *id = (map
                                            .map_eid_server_to_proxy(server.id, found_id - 1)
                                            + 1)
                                        .into();
                                        // docs dont say +1, same as above
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    (vec![], vec![])
                }
                PacketLatest::PlayDestroyEntity(ref mut body) => {
                    // since we're removing the id from the mapping table here, we have to map them here as well
                    let server_eid = *body.entity_id;
                    body.entity_id = map
                        .map_eid_server_to_proxy(server.id, *body.entity_id)
                        .into();
                    if let Some((proxy_eid, _)) = map.eids.remove_by_right(&(server.id, server_eid))
                    {
                        // debug!("destroying map s->p {} to {}", server_eid, proxy_eid);
                        map.entity_data.remove(&proxy_eid);
                        map.eid_gen.return_id(proxy_eid as u64);
                    }
                    (vec![], vec![])
                }
                _ => unreachable!(),
            };
            for id in nums {
                *id = map.map_eid_server_to_proxy(server.id, *id);
            }
            for id in varnums {
                *id = map.map_eid_server_to_proxy(server.id, **id).into();
            }
            if let Some(mut data) = entity_data {
                let proxy_eid = map.map_eid_server_to_proxy(server.id, data.id); // this should get the same id we just generated
                data.id = proxy_eid;
                map.entity_data.insert(proxy_eid, data);
            }
        }
        PacketDirection::ServerBound => {
            let eid = match packet {
                PacketLatest::PlayQueryEntityNbt(body) => &mut body.entity_id,
                PacketLatest::PlayInteractEntity(body) => &mut body.entity_id,
                PacketLatest::PlayEntityAction(body) => &mut body.entity_id,
                PacketLatest::PlayUpdateCommandBlockMinecart(body) => &mut body.entity_id,
                _ => unreachable!(),
            };
            if let Ok((server_id, server_eid)) = map.map_eid_proxy_to_server(**eid) {
                *eid = server_eid.into();
                return Some(server_id);
            }
        }
    };
    None
}
