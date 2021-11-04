use super::{
    PacketDestination,
    RelayPass,
};
use crate::{
    current::{
        proto::{
            EntityMetadataFieldData,
            SculkDestinationIdentifier,
        },
        protocol::PacketDirection,
        types::VarInt,
        PacketLatest,
        PacketLatestKind,
    },
    mapping::{
        EntityData,
        SplinterMapping,
        SplinterMappingResult,
    },
    server::SplinterServer,
};

inventory::submit! {
    RelayPass(Box::new(|proxy, connection, _client, sender, lazy_packet, destination| {
        if has_eids(lazy_packet.kind()) {
            if let Ok(packet) = lazy_packet.packet() {
                let map = &mut *smol::block_on(proxy.mapping.lock());
                match map_eid(map, packet, sender, &connection.server) {
                    SplinterMappingResult::Server(server_id) => {
                        *destination = PacketDestination::Server(server_id);
                        debug!("mapping packet {:?} to server {}", lazy_packet.kind(), server_id);
                    }
                    SplinterMappingResult::None => {
                        *destination = PacketDestination::None;
                        debug!("refusing to send packet of kind {:?} (no eid mapping)", packet);
                    }
                    _ => {}
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
            | PacketLatestKind::PlayDestroyEntities
            | PacketLatestKind::PlayQueryEntityNbt
            | PacketLatestKind::PlayInteractEntity
            | PacketLatestKind::PlayEntityAction
            | PacketLatestKind::PlayUpdateCommandBlockMinecart
            | PacketLatestKind::PlaySculkVibrationSignal
    )
}

pub fn map_eid(
    map: &mut SplinterMapping,
    packet: &mut PacketLatest,
    sender: &PacketDirection,
    server: &SplinterServer,
) -> SplinterMappingResult {
    match sender {
        PacketDirection::ClientBound => {
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
                PacketLatest::PlaySculkVibrationSignal(body) => {
                    if let SculkDestinationIdentifier::Entity(ref mut eid) = body.destination {
                        (vec![], vec![eid])
                    } else {
                        (vec![], vec![])
                    }
                }
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
                    body.entity_id = map.register_eid_mapping(server.id, *body.entity_id).into();
                    // debug!("entity spawn type: {}", entity_type);
                    (
                        match entity_type {
                            112 => {
                                // bobber
                                vec![&mut body.data]
                            }
                            2 | 84 | 43 | 81 | 16 | 104 => {
                                // arrow, spectral arrow, fireball, small fireball, dragon fireball, wither skull
                                if body.data > 0 {
                                    // body.data is option varint. we need to specially handle this
                                    if let Some(mapped_id) =
                                        map.eids.get_by_right(&(server.id, body.data - 1))
                                    {
                                        body.data = mapped_id + 1;
                                    } else {
                                        return SplinterMappingResult::None;
                                    }
                                }
                                vec![]
                            }
                            _ => {
                                // debug!("got type entity spawn type without eid {}", entity_type);
                                vec![]
                            }
                        },
                        vec![],
                    )
                }
                PacketLatest::PlaySpawnExperienceOrb(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 25,
                    });
                    body.entity_id = map.register_eid_mapping(server.id, *body.entity_id).into();
                    (vec![], vec![])
                }
                PacketLatest::PlaySpawnLivingEntity(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: *body.entity_type,
                    });
                    body.entity_id = map.register_eid_mapping(server.id, *body.entity_id).into();
                    (vec![], vec![])
                }
                PacketLatest::PlaySpawnPainting(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 60,
                    });
                    body.entity_id = map.register_eid_mapping(server.id, *body.entity_id).into();
                    (vec![], vec![])
                }
                PacketLatest::PlaySpawnPlayer(body) => {
                    entity_data = Some(EntityData {
                        id: *body.entity_id,
                        entity_type: 111,
                    });
                    body.entity_id = if let Some(mapped_id) =
                        map.eids.get_by_right(&(server.id, *body.entity_id))
                    {
                        (*mapped_id).into()
                    } else {
                        map.register_eid_mapping(server.id, *body.entity_id).into()
                        // for whatever reason, server has two eids per player or something. im
                        // not sure. this fixes it though
                    };
                    // debug!(
                    // "map player from {} to {}",
                    // entity_data.as_ref().unwrap().id,
                    // body.entity_id
                    // );
                    (vec![], vec![])
                }
                // complex
                PacketLatest::PlayEntityMetadata(body) => {
                    // we specially need to handle mapping here for the proxy side eid
                    let proxy_eid = if let Some(proxy_eid) =
                        map.eids.get_by_right(&(server.id, *body.entity_id))
                    {
                        *proxy_eid
                    } else {
                        return SplinterMappingResult::None;
                    };
                    body.entity_id = proxy_eid.into();
                    if let Some(data) = map.entity_data.get(&proxy_eid) {
                        match data.entity_type {
                            28 => {
                                // fireworks
                                if let Some(EntityMetadataFieldData::OptVarInt(ref mut id)) =
                                    body.metadata.get_mut(9)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        if let Some(mapped_id) =
                                            map.eids.get_by_right(&(server.id, found_id - 1))
                                        {
                                            *id = (mapped_id + 1).into();
                                        } else {
                                            return SplinterMappingResult::None;
                                        }
                                    }
                                }
                            }
                            112 => {
                                // fishing hook
                                if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                    body.metadata.get_mut(8)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        if let Some(mapped_id) =
                                            map.eids.get_by_right(&(server.id, found_id - 1))
                                        {
                                            *id = (mapped_id + 1).into();
                                        } else {
                                            return SplinterMappingResult::None;
                                        }
                                    }
                                }
                            }
                            102 => {
                                // wither
                                for index in [16, 17, 18] {
                                    if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                        body.metadata.get_mut(index)
                                    {
                                        let found_id: i32 = **id;
                                        if found_id > 0 {
                                            if let Some(mapped_id) =
                                                map.eids.get_by_right(&(server.id, found_id - 1))
                                            {
                                                *id = (mapped_id + 1).into(); // docs dont say + 1, but Im assuming that is the case here
                                            } else {
                                                return SplinterMappingResult::None;
                                            }
                                        }
                                    }
                                }
                            }
                            35 | 18 => {
                                // guardian or elder guardian
                                if let Some(EntityMetadataFieldData::VarInt(ref mut id)) =
                                    body.metadata.get_mut(17)
                                {
                                    let found_id: i32 = **id;
                                    if found_id > 0 {
                                        if let Some(mapped_id) =
                                            map.eids.get_by_right(&(server.id, found_id - 1))
                                        {
                                            *id = (mapped_id + 1).into(); // docs dont say +1, same as above
                                        } else {
                                            return SplinterMappingResult::None;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    (vec![], vec![])
                }
                PacketLatest::PlayDestroyEntities(ref mut body) => {
                    for eid in body.entity_ids.iter_mut() {
                        // since we're removing the id from the mapping table here, we have to map them here as well
                        let server_eid = **eid;
                        *eid = if let Some(mapped_id) = map.eids.get_by_right(&(server.id, **eid)) {
                            (*mapped_id).into()
                        } else {
                            return SplinterMappingResult::None;
                        };
                        if let Some((proxy_eid, _)) =
                            map.eids.remove_by_right(&(server.id, server_eid))
                        {
                            debug!(
                                "destroying map s->p ({}, {}) to {}",
                                server.id, server_eid, proxy_eid
                            );
                            map.entity_data.remove(&proxy_eid);
                            map.eid_gen.return_id(proxy_eid as u64);
                        }
                    }
                    (vec![], vec![])
                }
                _ => unreachable!(),
            };
            for id in nums {
                *id = if let Some(mapped_id) = map.eids.get_by_right(&(server.id, *id)) {
                    *mapped_id
                } else {
                    return SplinterMappingResult::None;
                };
            }
            for id in varnums {
                *id = if let Some(mapped_id) = map.eids.get_by_right(&(server.id, **id)) {
                    (*mapped_id).into()
                } else {
                    return SplinterMappingResult::None;
                };
            }
            if let Some(mut data) = entity_data {
                let proxy_eid =
                    if let Some(mapped_id) = map.eids.get_by_right(&(server.id, data.id)) {
                        *mapped_id
                    } else {
                        return SplinterMappingResult::None;
                    }; // this should get the same id we just generated
                data.id = proxy_eid;
                map.entity_data.insert(proxy_eid, data);
            }
            return SplinterMappingResult::Client;
        }
        PacketDirection::ServerBound => {
            let eid = match packet {
                PacketLatest::PlayQueryEntityNbt(body) => &mut body.entity_id,
                PacketLatest::PlayInteractEntity(body) => &mut body.entity_id,
                PacketLatest::PlayEntityAction(body) => &mut body.entity_id,
                PacketLatest::PlayUpdateCommandBlockMinecart(body) => &mut body.entity_id,
                _ => unreachable!(),
            };
            if let Some((server_id, server_eid)) = map.eids.get_by_left(&**eid) {
                *eid = (*server_eid).into();
                return SplinterMappingResult::Server(*server_id);
            }
        }
    };
    return SplinterMappingResult::None;
}
