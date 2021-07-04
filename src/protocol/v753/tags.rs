use std::{
    collections::HashMap,
    iter::FromIterator,
};

use bimap::BiHashMap;
use mcproto_rs::{
    types::{
        CountedArray,
        VarInt,
    },
    v1_16_3::{
        PlayTagsSpec,
        TagSpec,
    },
};

use crate::protocol::{
    load_json_id_name_pairs,
    TagList,
    Tags,
};

lazy_static! {
    pub static ref BLOCK_MAP: BiHashMap<i32, String> =
        BiHashMap::<i32, String>::from_iter(load_json_id_name_pairs(include_str!(
            "../../../minecraft-data/data/pc/1.16.2/blocks.json"
        )));
    pub static ref ITEM_MAP: BiHashMap<i32, String> =
        BiHashMap::<i32, String>::from_iter(load_json_id_name_pairs(include_str!(
            "../../../minecraft-data/data/pc/1.16.2/items.json"
        )));
    pub static ref ENTITY_MAP: BiHashMap<i32, String> =
        BiHashMap::<i32, String>::from_iter(load_json_id_name_pairs(include_str!(
            "../../../minecraft-data/data/pc/1.16.2/entities.json"
        )));
    pub static ref FLUID_MAP: BiHashMap<i32, String> = BiHashMap::<i32, String>::from_iter(
        load_json_id_name_pairs(include_str!("../../../fluids.json")),
    );
}

pub fn proto_tags_to_tags(
    proto_tags: &CountedArray<TagSpec, VarInt>,
    map: &BiHashMap<i32, String>,
) -> TagList {
    let mut list = HashMap::new();
    for tag in proto_tags.iter() {
        list.insert(
            tag.name.clone(),
            tag.entries
                .iter()
                .map(|val| map.get_by_left(&**val).unwrap().clone())
                .collect::<Vec<String>>(),
        );
    }
    TagList(list)
}

pub fn tags_to_proto_tags(
    tags: &TagList,
    map: &BiHashMap<i32, String>,
) -> CountedArray<TagSpec, VarInt> {
    let mut list = vec![];
    for (name, ids) in tags.0.iter() {
        list.push(TagSpec {
            name: name.clone(),
            entries: ids
                .iter()
                .map(|id| VarInt::from(*map.get_by_right(id).unwrap()))
                .collect::<Vec<VarInt>>()
                .into(),
        });
    }
    list.into()
}

impl From<&PlayTagsSpec> for Tags {
    fn from(proto_tags: &PlayTagsSpec) -> Tags {
        let mut tags = HashMap::new();
        tags.insert(
            "minecraft:block".into(),
            proto_tags_to_tags(&proto_tags.block_tags, &BLOCK_MAP),
        );
        tags.insert(
            "minecraft:item".into(),
            proto_tags_to_tags(&proto_tags.item_tags, &ITEM_MAP),
        );
        tags.insert(
            "minecraft:fluid".into(),
            proto_tags_to_tags(&proto_tags.fluid_tags, &FLUID_MAP),
        );
        tags.insert(
            "minecraft:entity_type".into(),
            proto_tags_to_tags(&proto_tags.entity_tags, &ENTITY_MAP),
        );
        Tags {
            tags,
        }
    }
}

impl From<&Tags> for PlayTagsSpec {
    fn from(tags: &Tags) -> PlayTagsSpec {
        PlayTagsSpec {
            block_tags: tags_to_proto_tags(
                &tags.tags.get("minecraft:block".into()).unwrap(),
                &BLOCK_MAP,
            ),
            item_tags: tags_to_proto_tags(
                &tags.tags.get("minecraft:item".into()).unwrap(),
                &ITEM_MAP,
            ),
            fluid_tags: tags_to_proto_tags(
                &tags.tags.get("minecraft:fluid".into()).unwrap(),
                &FLUID_MAP,
            ),
            entity_tags: tags_to_proto_tags(
                &tags.tags.get("minecraft:entity_type".into()).unwrap(),
                &ENTITY_MAP,
            ),
        }
    }
}
