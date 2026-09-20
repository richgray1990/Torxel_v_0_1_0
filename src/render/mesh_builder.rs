//! Построение мешей субчанков.

use bevy::prelude::*;

use crate::voxel::format::{
    CHUNK_HEIGHT, CHUNK_SIDE, SUBCHUNK_HEIGHT, SUBCHUNK_SIDE,
    SUBCHUNKS_X, SUBCHUNKS_Y,
};
use crate::voxel::materials::{get_material, ids, is_transparent};
use crate::voxel::pool::ChunkPool;

pub const MAX_VERTICES_PER_SUBCHUNK: usize =
    SUBCHUNK_SIDE * SUBCHUNK_HEIGHT * SUBCHUNK_SIDE * 6 * 4;

pub const MAX_INDICES_PER_SUBCHUNK: usize =
    SUBCHUNK_SIDE * SUBCHUNK_HEIGHT * SUBCHUNK_SIDE * 6 * 6;

const FACES: [(i64, i64, i64); 6] = [
    (0, 1, 0), (0, -1, 0),
    (1, 0, 0), (-1, 0, 0),
    (0, 0, 1), (0, 0, -1),
];

const FACE_NORMALS: [[f32; 3]; 6] = [
    [0.0, 1.0, 0.0], [0.0, -1.0, 0.0],
    [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0], [0.0, 0.0, -1.0],
];

const FACE_VERTICES: [[[f32; 3]; 4]; 6] = [
    [[0.0,1.0,0.0],[1.0,1.0,0.0],[0.0,1.0,1.0],[1.0,1.0,1.0]],
    [[0.0,0.0,0.0],[0.0,0.0,1.0],[1.0,0.0,0.0],[1.0,0.0,1.0]],
    [[1.0,0.0,0.0],[1.0,1.0,0.0],[1.0,0.0,1.0],[1.0,1.0,1.0]],
    [[0.0,0.0,0.0],[0.0,0.0,1.0],[0.0,1.0,0.0],[0.0,1.0,1.0]],
    [[0.0,0.0,1.0],[1.0,0.0,1.0],[0.0,1.0,1.0],[1.0,1.0,1.0]],
    [[0.0,0.0,0.0],[0.0,1.0,0.0],[1.0,0.0,0.0],[1.0,1.0,0.0]],
];

const FACE_UVS: [[f32; 2]; 4] = [
    [0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0],
];

const FACE_INDICES: [u32; 6] = [0, 1, 2, 2, 1, 3];

pub struct SubchunkMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl SubchunkMeshData {
    pub fn new() -> Self {
        Self {
            positions: Vec::with_capacity(MAX_VERTICES_PER_SUBCHUNK),
            normals: Vec::with_capacity(MAX_VERTICES_PER_SUBCHUNK),
            uvs: Vec::with_capacity(MAX_VERTICES_PER_SUBCHUNK),
            colors: Vec::with_capacity(MAX_VERTICES_PER_SUBCHUNK),
            indices: Vec::with_capacity(MAX_INDICES_PER_SUBCHUNK),
        }
    }

    pub fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.uvs.clear();
        self.colors.clear();
        self.indices.clear();
    }
}

impl Default for SubchunkMeshData {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_subchunk_mesh(
    subchunk_index: usize,
    pool: &ChunkPool,
    slot: usize,
    _world_width: i64,
    _world_depth: i64,
) -> SubchunkMeshData {
    let mut mesh_data = SubchunkMeshData::new();

    let sub_x = (subchunk_index % SUBCHUNKS_X) * SUBCHUNK_SIDE;
    let sub_y = ((subchunk_index / SUBCHUNKS_X) % SUBCHUNKS_Y) * SUBCHUNK_HEIGHT;
    let sub_z = (subchunk_index / (SUBCHUNKS_X * SUBCHUNKS_Y)) * SUBCHUNK_SIDE;

    for lz in sub_z..sub_z + SUBCHUNK_SIDE {
        for lx in sub_x..sub_x + SUBCHUNK_SIDE {
            for ly in sub_y..sub_y + SUBCHUNK_HEIGHT {
                let cell_index = (lz * CHUNK_SIDE + lx) * CHUNK_HEIGHT + ly;
                let cell = pool.get(slot, cell_index);

                if cell.material_id == ids::AIR {
                    continue;
                }

                let material = match get_material(cell.material_id) {
                    Some(m) => m,
                    None => continue,
                };

                for (face_idx, (dx, dy, dz)) in FACES.iter().enumerate() {
                    let nx = lx as i64 + dx;
                    let ny = ly as i64 + dy;
                    let nz = lz as i64 + dz;

                    let visible = is_face_visible(nx, ny, nz, pool, slot);

                    if visible {
                        let brightness = 1.0 - (ly as f32 / CHUNK_HEIGHT as f32) * 0.5;
                        let r = material.color[0] as f32 / 255.0 * brightness;
                        let g = material.color[1] as f32 / 255.0 * brightness;
                        let b = material.color[2] as f32 / 255.0 * brightness;
                        let a = material.opacity;

                        let normal = FACE_NORMALS[face_idx];
                        let face_verts = FACE_VERTICES[face_idx];
                        let base_index = mesh_data.positions.len() as u32;

                        for (v_idx, vertex) in face_verts.iter().enumerate() {
                            mesh_data.positions.push([
                                lx as f32 + vertex[0],
                                ly as f32 + vertex[1],
                                lz as f32 + vertex[2],
                            ]);
                            mesh_data.normals.push(normal);
                            mesh_data.uvs.push(FACE_UVS[v_idx]);
                            mesh_data.colors.push([r, g, b, a]);
                        }

                        for idx in FACE_INDICES.iter() {
                            mesh_data.indices.push(base_index + idx);
                        }
                    }
                }
            }
        }
    }

    mesh_data
}

fn is_face_visible(
    neighbor_x: i64,
    neighbor_y: i64,
    neighbor_z: i64,
    pool: &ChunkPool,
    slot: usize,
) -> bool {
    if neighbor_y < 0 {
        return false;
    }
    if neighbor_y >= CHUNK_HEIGHT as i64 {
        return true;
    }
    if neighbor_x < 0 || neighbor_x >= CHUNK_SIDE as i64 {
        return false;
    }
    if neighbor_z < 0 || neighbor_z >= CHUNK_SIDE as i64 {
        return false;
    }

    let idx = (neighbor_z as usize * CHUNK_SIDE + neighbor_x as usize) * CHUNK_HEIGHT
        + neighbor_y as usize;
    let neighbor_cell = pool.get(slot, idx);

    is_transparent(neighbor_cell.material_id)
}