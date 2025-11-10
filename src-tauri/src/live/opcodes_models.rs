use blueprotobuf_lib::blueprotobuf::{EEntityType, SyncContainerData};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

pub type EncounterMutex = Mutex<Encounter>;

#[derive(Debug, Default, Clone)]
pub struct Encounter {
    pub is_encounter_paused: bool,
    pub time_last_combat_packet_ms: u128,
    pub time_fight_start_ms: u128,
    pub local_player_uid: Option<i64>,
    pub entity_uid_to_entity: HashMap<i64, Entity>,
    pub dmg_stats: CombatStats,
    pub local_player: Option<SyncContainerData>,
    pub crowdsource_monster_name: Option<String>,
    pub crowdsource_monster_id: Option<i32>,
    pub crowdsource_monster_remote_id: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Entity {
    pub entity_type: EEntityType,
    pub name: Option<String>, 
    pub monster_id: Option<i32>,
    pub curr_hp: Option<i32>, 
    pub max_hp: Option<i32>,
}

#[derive(Debug, Default, Clone)]
pub struct CombatStats {
    pub value: i64,
}

pub static MONSTER_NAMES: Lazy<HashMap<i32, String>> = Lazy::new(|| {
    let data = include_str!("../../../src/lib/data/json/MonsterName.json");
    serde_json::from_str(data).expect("invalid MonsterName.json")
});

pub static MONSTER_NAMES_CROWDSOURCE: Lazy<HashMap<i32, String>> = Lazy::new(|| {
    let data = include_str!("../../../src/lib/data/json/MonsterNameCrowdsource.json");
    serde_json::from_str(data).expect("invalid MonsterName.json")
});

pub static MONSTER_UID_CROWDSOURCE_MAP: Lazy<HashMap<i32, String>> = Lazy::new(|| {
    let raw: HashMap<String, String> = serde_json::from_str(include_str!(
        "../../../src/lib/data/json/MonsterUidCrowdsource.json"
    ))
    .expect("Failed to parse MonsterUidCrowdsource.json");

    raw.into_iter()
        .filter_map(|(id_str, remote_id)| {
            id_str
                .parse::<i32>()
                .ok()
                .map(|id| (id, remote_id))
        })
        .collect()
});

static MONSTER_REMOTE_LOOKUP: Lazy<HashMap<String, (i32, String)>> = Lazy::new(|| {
    let mut pairs = MONSTER_UID_CROWDSOURCE_MAP
        .iter()
        .map(|(id, remote)| (*id, remote.clone()))
        .collect::<Vec<_>>();
    pairs.sort_by_key(|(id, _)| *id);

    let mut map: HashMap<String, (i32, String)> = HashMap::new();

    for (id, remote_id) in pairs {
        let name = MONSTER_NAMES_CROWDSOURCE
            .get(&id)
            .or_else(|| MONSTER_NAMES.get(&id))
            .cloned()
            .unwrap_or_else(|| format!("Monster {id}"));

        map.entry(remote_id.clone())
            .and_modify(|entry| {
                let current_name = &entry.1;
                let current_has_suffix = current_name.contains('-');
                let new_has_suffix = name.contains('-');

                if (current_has_suffix && !new_has_suffix)
                    || (current_has_suffix == new_has_suffix && id < entry.0)
                {
                    *entry = (id, name.clone());
                }
            })
            .or_insert((id, name));
    }

    map
});

pub fn get_crowdsource_monster_choices() -> Vec<(i32, String, String)> {
    let mut choices: Vec<(i32, String, String)> = MONSTER_REMOTE_LOOKUP
        .iter()
        .map(|(remote_id, (monster_id, name))| (*monster_id, name.clone(), remote_id.clone()))
        .collect();

    choices.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    choices
}

pub fn resolve_crowdsource_remote(remote_id: &str) -> Option<(i32, String)> {
    MONSTER_REMOTE_LOOKUP
        .get(remote_id)
        .map(|(monster_id, name)| (*monster_id, name.clone()))
}

pub mod attr_type {
    pub const ATTR_NAME: i32 = 0x01;
    pub const ATTR_ID: i32 = 0x0a;
    pub const ATTR_HP: i32 = 0x2c2e;
    pub const ATTR_MAX_HP: i32 = 0x2c38;
}
