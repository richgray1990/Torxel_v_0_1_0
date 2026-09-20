//! Применение событий ячеек к WriteWorld.

use bevy::prelude::*;

use crate::voxel::pool::WriteWorld;
use crate::queues::cell_events::CellEventQueue;

/// Система применения событий ячеек
pub fn apply_cell_events_system(
    mut write_world: ResMut<WriteWorld>,
    mut cell_events: ResMut<CellEventQueue>,
) {
    for event in cell_events.drain() {
        let slot = event.slot_index;
        let cell_idx = event.cell_index;

        // Записываем ячейку в WriteWorld
        *write_world.0.get_mut(slot, cell_idx) = event.cell;
    }
}