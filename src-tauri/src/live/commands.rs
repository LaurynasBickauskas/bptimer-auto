use crate::live::commands_models::HeaderInfo;
use crate::live::crowdsource_persistence::{save_snapshot, CrowdsourceMonsterSnapshot};
use crate::live::opcodes_models::{
    get_crowdsource_monster_choices, resolve_crowdsource_remote, Encounter, EncounterMutex,
};
use crate::live::bptimer_stream::{BPTIMER_BASE_URL, CREATE_HP_REPORT_ENDPOINT, CROWD_SOURCE_API_KEY};
use crate::packets::packet_capture::request_restart;
use crate::WINDOW_LIVE_LABEL;
use log::{info, warn};
use reqwest::Client;
use serde::Serialize;
use specta::Type;
use tauri::Manager;
use window_vibrancy::{apply_blur, clear_blur};

#[tauri::command]
#[specta::specta]
pub fn enable_blur(app: tauri::AppHandle) {
    if let Some(meter_window) = app.get_webview_window(WINDOW_LIVE_LABEL) {
        apply_blur(&meter_window, Some((10, 10, 10, 50))).ok();
    }
}

#[tauri::command]
#[specta::specta]
pub fn disable_blur(app: tauri::AppHandle) {
    if let Some(meter_window) = app.get_webview_window(WINDOW_LIVE_LABEL) {
        clear_blur(&meter_window).ok();
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct CrowdsourcedMonster {
    pub name: String,
    pub id: i32,
    pub remote_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct CrowdsourcedMonsterOption {
    pub name: String,
    pub id: i32,
    pub remote_id: String,
}

#[tauri::command]
#[specta::specta]
pub fn get_last_hit_boss_name(state: tauri::State<'_, EncounterMutex>) -> Option<String> {
    let encounter = state.lock().unwrap();
    let result = encounter.crowdsource_monster_name.clone();
    result
}

#[tauri::command]
#[specta::specta]
pub fn get_crowdsourced_monster(state: tauri::State<'_, EncounterMutex>) -> Option<CrowdsourcedMonster> {
    let encounter = state.lock().unwrap();
    match (
        &encounter.crowdsource_monster_name,
        encounter.crowdsource_monster_id,
        encounter.crowdsource_monster_remote_id.as_ref(),
    ) {
        (Some(name), Some(id), remote_id) => Some(CrowdsourcedMonster {
            name: name.clone(),
            id,
             remote_id: remote_id.cloned(),
        }),
        _ => None,
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_crowdsourced_monster_options() -> Vec<CrowdsourcedMonsterOption> {
    get_crowdsource_monster_choices()
        .into_iter()
        .map(|(id, name, remote_id)| CrowdsourcedMonsterOption {
            name,
            id,
            remote_id,
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn set_crowdsourced_monster_remote(
    app: tauri::AppHandle,
    encounter_state: tauri::State<'_, EncounterMutex>,
    remote_id: String,
) -> Result<(), String> {
    let (monster_id, monster_name) =
        resolve_crowdsource_remote(&remote_id).ok_or_else(|| format!("Unknown remote id: {remote_id}"))?;

    {
        let mut encounter = encounter_state
            .lock()
            .map_err(|_| "Failed to lock encounter".to_string())?;
        encounter.crowdsource_monster_id = Some(monster_id);
        encounter.crowdsource_monster_name = Some(monster_name.clone());
        encounter.crowdsource_monster_remote_id = Some(remote_id.clone());
    }

    let snapshot = CrowdsourceMonsterSnapshot {
        monster_id,
        monster_name,
        remote_id: remote_id.clone(),
    };

    if let Err(err) = save_snapshot(&app, &snapshot) {
        warn!(
            "commands::set_crowdsourced_monster_remote - failed to persist snapshot for remote_id={remote_id}: {err}"
        );
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_local_player_line(state: tauri::State<'_, EncounterMutex>) -> Result<Option<i32>, String> {
    let encounter = state
        .lock()
        .map_err(|_| "Failed to lock encounter".to_string())?;

    let line = encounter
        .local_player
        .as_ref()
        .and_then(|player| player.v_data.as_ref())
        .and_then(|v| v.scene_data.as_ref())
        .and_then(|scene| scene.line_id);

    Ok(line.map(|line| line as i32))
}

#[tauri::command]
#[specta::specta]
pub async fn mark_current_crowdsourced_line_dead(
    encounter_state: tauri::State<'_, EncounterMutex>,
) -> Result<(), String> {
    let (monster_id, monster_name, line, pos_x, pos_y) = {
        let encounter = encounter_state
            .lock()
            .map_err(|_| "Failed to lock encounter".to_string())?;

        let monster_id = encounter
            .crowdsource_monster_id
            .ok_or_else(|| "No crowdsourced monster ID available".to_string())?;

        let monster_name = encounter
            .crowdsource_monster_name
            .clone()
            .unwrap_or_else(|| "Unknown Monster".to_string());

        let scene_data = encounter
            .local_player
            .as_ref()
            .and_then(|player| player.v_data.as_ref())
            .and_then(|v| v.scene_data.as_ref())
            .ok_or_else(|| "No local player scene data available".to_string())?;

        let line = scene_data
            .line_id
            .ok_or_else(|| "No line id available for local player".to_string())?;

        let pos = scene_data
            .pos
            .as_ref()
            .ok_or_else(|| "No position data available for local player".to_string())?;

        let pos_x = pos
            .x
            .ok_or_else(|| "No pos_x available for local player".to_string())? as f64;

        let pos_y = pos
            .y
            .ok_or_else(|| "No pos_y available for local player".to_string())? as f64;

        (monster_id, monster_name, line, pos_x, pos_y)
    };

    info!(
        "mark_current_crowdsourced_line_dead - reporting monster '{}' ({}) as dead on line {}",
        monster_name, monster_id, line
    );

    let body = serde_json::json!({
        "monster_id": monster_id,
        "hp_pct": 0,
        "line": line,
        "pos_x": pos_x,
        "pos_y": pos_y,
    });

    let client = Client::new();
    let response = client
        .post(format!("{BPTIMER_BASE_URL}{CREATE_HP_REPORT_ENDPOINT}"))
        .header("X-API-Key", CROWD_SOURCE_API_KEY)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("Failed to send HP report: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "HP report failed with status {}",
            response.status()
        ));
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_header_info(state: tauri::State<'_, EncounterMutex>) -> Result<HeaderInfo, String> {
    let encounter = state.lock().unwrap();
    if encounter.dmg_stats.value == 0 {
        return Err("No damage found".to_string());
    }

    let time_elapsed_ms = encounter.time_last_combat_packet_ms - encounter.time_fight_start_ms;

    let encounter_stats = &encounter.dmg_stats;

    Ok(HeaderInfo {
        total_dmg: encounter_stats.value as f64,
        elapsed_ms: time_elapsed_ms as f64,
        time_last_combat_packet_ms: encounter.time_last_combat_packet_ms as f64,
    })
}

#[tauri::command]
#[specta::specta]
pub fn hard_reset(state: tauri::State<'_, EncounterMutex>) {
    let mut encounter = state.lock().unwrap();
    encounter.clone_from(&Encounter::default());
    request_restart();
    info!("Hard Reset");
}

#[tauri::command]
#[specta::specta]
pub fn reset_encounter(state: tauri::State<'_, EncounterMutex>) {
    let mut encounter = state.lock().unwrap();
    encounter.clone_from(&Encounter::default());
    info!("encounter reset");
}

#[tauri::command]
#[specta::specta]
pub fn toggle_pause_encounter(state: tauri::State<'_, EncounterMutex>) {
    let mut encounter = state.lock().unwrap();
    encounter.is_encounter_paused = !encounter.is_encounter_paused;
}
