//! Взаимодействие с блоками через Aggregate.
//!
//! R — поставить камень сверху на блок под курсором.
//! T — удалить блок под курсором.
//!
//! Без Arc, без Atomic, без unsafe.
//! Все изменения идут только через CellEventQueue.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bytemuck::Zeroable;

use crate::manager::ReadWindowManager;
use crate::queues::cell_events::{CellEvent, CellEventQueue};
use crate::render::camera::CameraController;
use crate::voxel::format::{Cell, CHUNK_HEIGHT, CHUNK_SIDE, WorldDimensions};
use crate::voxel::materials::{ids, phases};
use crate::voxel::pool::ReadWorld;

/// Максимальная дальность луча в блоках.
const MAX_RAY_DISTANCE: f64 = 4096.0;

#[derive(Clone, Copy, Debug)]
struct VoxelHit {
    slot: usize,
    lx: usize,
    ly: usize,
    lz: usize,
    cell_index: usize,
}

pub fn block_interaction_system(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    controller: Res<CameraController>,
    dimensions: Res<WorldDimensions>,
    read_world: Res<ReadWorld>,
    read_window: Res<ReadWindowManager>,
    mut events: ResMut<CellEventQueue>,
) {
    let remove_pressed = keys.just_pressed(KeyCode::KeyT);
    let place_pressed = keys.just_pressed(KeyCode::KeyR);

    if !remove_pressed && !place_pressed {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        return;
    };
    print!("ray.origin.x = {},", ray.origin.x);
    print!("ray.origin.z = {},", ray.origin.z);
    // Камера локальная: её X/Z живут вокруг нуля.
    // Возвращаем луч в мировые координаты.
    // Wrap нужен, чтобы не гонять DDA по гигантским i64 после долгого полёта.
    let origin_x = dimensions.wrap_x(controller.anchor_x + ray.origin.x as f64);
    let origin_y = ray.origin.y as f64;
    let origin_z = dimensions.wrap_z(controller.anchor_z + ray.origin.z as f64);
    print!("origin_x = {},", origin_x);
    print!("origin_y = {},", origin_y);
    print!("origin_z = {}. ", origin_z);

    let dir_x = ray.direction.x as f64;
    let dir_y = ray.direction.y as f64;
    let dir_z = ray.direction.z as f64;

    print!("dir_x = {},", dir_x);
    print!("dir_y = {},", dir_y);
    print!("dir_z = {}\n", dir_z);

    let Some(hit) = voxel_ray_cast(
        origin_x,
        origin_y,
        origin_z,
        dir_x,
        dir_y,
        dir_z,
        MAX_RAY_DISTANCE,
        &read_world,
        &read_window,
    ) else {
        println!("return at some(hit)");
        return;
    };

    let generation = read_window.generation(hit.slot);

    // T — удалить блок, в который смотрим.
    if remove_pressed {
        let mut cell = Cell::zeroed();
        cell.material_id = ids::AIR;
        cell.phase_state = phases::SOLID;
        println!("event push: remove!");

        events.push(CellEvent::set_cell_expected(
            hit.slot,
            hit.cell_index,
            cell,
            generation,
        ));
    }

    // R — поставить камень сверху над блоком, в который смотрим.
    if place_pressed {
        println!("event push: build stone!");
        let place_ly = hit.ly + 1;

        // Вертикальных чанков пока нет, выше потолка ставить некуда.
        if place_ly >= CHUNK_HEIGHT {
            return;
        }

        let place_cell_index = (hit.lz * CHUNK_SIDE + hit.lx) * CHUNK_HEIGHT + place_ly;

        // Ставим только в воздух.
        if read_world.0.get(hit.slot, place_cell_index).material_id != ids::AIR {
            return;
        }

        let mut cell = Cell::zeroed();
        cell.material_id = ids::STONE;
        cell.phase_state = phases::SOLID;

        events.push(CellEvent::set_cell_expected(
            hit.slot,
            place_cell_index,
            cell,
            generation,
        ));
    }
}

fn voxel_ray_cast(
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    max_dist: f64,
    read_world: &ReadWorld,
    read_window: &ReadWindowManager,
) -> Option<VoxelHit> {
    let mut vx = ox.floor() as i64;
    let mut vy = oy.floor() as i64;
    let mut vz = oz.floor() as i64;

    // // Мир по Y конечен. Если луч горизонтальный и вне вертикального диапазона — выхода нет
    if dy == 0.0 && (vy < 0 || vy >= CHUNK_HEIGHT as i64) {
        return None;
    }

    let step_x = signum_i64(dx);
    let step_y = signum_i64(dy);
    let step_z = signum_i64(dz);

    let t_delta_x = if dx != 0.0 { (1.0 / dx).abs() } else { f64::INFINITY };
    let t_delta_y = if dy != 0.0 { (1.0 / dy).abs() } else { f64::INFINITY };
    let t_delta_z = if dz != 0.0 { (1.0 / dz).abs() } else { f64::INFINITY };

    let mut t_max_x = if step_x > 0 {
        ((vx + 1) as f64 - ox) / dx
    } else if step_x < 0 {
        (vx as f64 - ox) / dx
    } else {
        f64::INFINITY
    };

    let mut t_max_y = if step_y > 0 {
        ((vy + 1) as f64 - oy) / dy
    } else if step_y < 0 {
        (vy as f64 - oy) / dy
    } else {
        f64::INFINITY
    };

    let mut t_max_z = if step_z > 0 {
        ((vz + 1) as f64 - oz) / dz
    } else if step_z < 0 {
        (vz as f64 - oz) / dz
    } else {
        f64::INFINITY
    };

    let mut t = 0.0_f64;

    loop {
        if t > max_dist {
            println!("Return: t > max_dist");
            return None;
        }

       // Ниже мира.
        // Если летим вниз или горизонтально — дальше смысла нет.
        // Если летим вверх — ещё можем войти в мир.
        if vy < 0 {
            if step_y <= 0 {
                return None;
            }
        }
        // Выше мира.
        // Если летим вверх или горизонтально — дальше смысла нет.
        // Если летим вниз — ещё можем войти в мир.
        else if vy >= CHUNK_HEIGHT as i64 {
            if step_y >= 0 {
                return None;
            }
        }

        if let Some(hit) = check_voxel(vx, vy, vz, read_world, read_window) {
            println!("Return: let Some(hit) = check_voxel(vx, vy, vz, read_world, read_window)");
            return Some(hit);
        }

        if t_max_x <= t_max_y && t_max_x <= t_max_z {
            t = t_max_x;
            vx += step_x;
            t_max_x += t_delta_x;
        } else if t_max_y <= t_max_x && t_max_y <= t_max_z {
            t = t_max_y;
            vy += step_y;
            t_max_y += t_delta_y;
        } else {
            t = t_max_z;
            vz += step_z;
            t_max_z += t_delta_z;
        }
    }
}

fn check_voxel(
    wx: i64,
    wy: i64,
    wz: i64,
    read_world: &ReadWorld,
    read_window: &ReadWindowManager,
) -> Option<VoxelHit> {
    if wy < 0 || wy >= CHUNK_HEIGHT as i64 {
        return None;
    }

    let chunk_x_raw = wx.div_euclid(CHUNK_SIDE as i64);
    let chunk_z_raw = wz.div_euclid(CHUNK_SIDE as i64);

    let lx = wx.rem_euclid(CHUNK_SIDE as i64) as usize;
    let lz = wz.rem_euclid(CHUNK_SIDE as i64) as usize;
    let ly = wy as usize;

    let (cx, cz) = read_window.normalize_chunk(chunk_x_raw, chunk_z_raw);
    let slot = read_window.slot_for_chunk(cx, cz)?;

    if !read_window.is_ready(slot) {
        return None;
    }

    let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;

    if read_world.0.get(slot, cell_index).material_id == ids::AIR {
        return None;
    }

    Some(VoxelHit {
        slot,
        lx,
        ly,
        lz,
        cell_index,
    })
}

#[inline(always)]
fn signum_i64(v: f64) -> i64 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}