//! Построение мешей субчанков.

use bevy::prelude::*;

use crate::voxel::format::{
    CHUNK_HEIGHT, CHUNK_SIDE, SUBCHUNK_HEIGHT, SUBCHUNK_SIDE,
    SUBCHUNKS_X, SUBCHUNKS_Y,
};
use crate::voxel::materials::{get_material, ids, is_transparent};
use crate::voxel::pool::ChunkPool;

use crate::manager::ReadWindowManager;

/// Теоретическая верхняя граница вершин в субчанке (все ячейки — отдельные кубы).
/// Не используется для резервирования: реальный субчанк заполнен на доли процента,
/// и резерв по этому потолку стоит ~5 МБ на вызов.
pub const MAX_VERTICES_PER_SUBCHUNK: usize =
    SUBCHUNK_SIDE * SUBCHUNK_HEIGHT * SUBCHUNK_SIDE * 6 * 4;

/// Теоретическая верхняя граница индексов в субчанке. См. `MAX_VERTICES_PER_SUBCHUNK`.
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
    [[0.0,1.0,0.0],[0.0,1.0,1.0],[1.0,1.0,0.0],[1.0,1.0,1.0]],  // Верхняя ← ИСПРАВЛЕНО
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
    /// Без предварительного резерва.
    ///
    /// Пустой субчанк тогда не аллоцирует вообще ничего, а их 31 из 32 (геометрия
    /// мира лежит в первой субчанковой полосе). Резерв по `MAX_*` стоил бы ~5 МБ
    /// на каждый вызов, то есть гигабайты churn за один сдвиг окна.
    pub fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
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

/// Сторона halo-буфера по горизонтали: субчанк + по одному блоку с каждой стороны.
pub const HALO_SIDE: usize = SUBCHUNK_SIDE + 2;
/// Высота halo-буфера: субчанк + по одному блоку снизу и сверху.
pub const HALO_HEIGHT: usize = SUBCHUNK_HEIGHT + 2;
/// Полное число ячеек halo (18³ = 5832).
pub const HALO_CELLS: usize = HALO_SIDE * HALO_SIDE * HALO_HEIGHT;

/// Сентинел «непроницаемо»: под `y = 0` грани не рисуются, поэтому сосед должен
/// считаться непрозрачным. `is_transparent` для неизвестного id вернёт `true`,
/// поэтому нужен именно существующий непрозрачный материал.
const HALO_OPAQUE: u8 = ids::BEDROCK;

/// Материалы субчанка вместе с одноблочным кольцом соседей по всем шести граням.
///
/// Заполняется один раз за вызов мешера; горячий цикл читает соседа по индексу
/// без ветвлений, обращений к чужим слотам пула и тороидальных нормализаций.
/// Хранит только `material_id` — цвет/AO соседа мешеру не нужны, а 18³×Cell
/// стоили бы 93 КБ вместо 5.7 КБ.
pub struct HaloBuf {
    cells: [u8; HALO_CELLS],
}

impl HaloBuf {
    pub const fn new() -> Self {
        Self { cells: [ids::AIR; HALO_CELLS] }
    }

    /// Локальные координаты halo: 0..18. Центр субчанка — 1..17.
    #[inline(always)]
    pub fn get(&self, hx: usize, hy: usize, hz: usize) -> u8 {
        self.cells[(hz * HALO_HEIGHT + hy) * HALO_SIDE + hx]
    }

    #[inline(always)]
    pub fn set(&mut self, hx: usize, hy: usize, hz: usize, material_id: u8) {
        self.cells[(hz * HALO_HEIGHT + hy) * HALO_SIDE + hx] = material_id;
    }
}

impl Default for HaloBuf {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_subchunk_mesh(
    halo: &mut HaloBuf,
    subchunk_index: usize,
    pool: &ChunkPool,
    //slot: usize,
    chunk_x: i64,
    chunk_z: i64,
    window: &ReadWindowManager,
) -> SubchunkMeshData {
    let mut mesh_data = SubchunkMeshData::new();

    let sub_x = (subchunk_index % SUBCHUNKS_X) * SUBCHUNK_SIDE;
    let sub_y = ((subchunk_index / SUBCHUNKS_X) % SUBCHUNKS_Y) * SUBCHUNK_HEIGHT;
    let sub_z = (subchunk_index / (SUBCHUNKS_X * SUBCHUNKS_Y)) * SUBCHUNK_SIDE;

    fill_halo(halo, pool, chunk_x, chunk_z, sub_x, sub_y, sub_z, window);

    for lz in sub_z..sub_z + SUBCHUNK_SIDE {
        for lx in sub_x..sub_x + SUBCHUNK_SIDE {
            for ly in sub_y..sub_y + SUBCHUNK_HEIGHT {
                // Локальные координаты внутри субчанка и соответствующие им +1 в halo.
                let hx = lx - sub_x + 1;
                let hy = ly - sub_y + 1;
                let hz = lz - sub_z + 1;

                let material_id = halo.get(hx, hy, hz);

                if material_id == ids::AIR {
                    continue;
                }

                let material = match get_material(material_id) {
                    Some(m) => m,
                    None => continue,
                };

                for (face_idx, (dx, dy, dz)) in FACES.iter().enumerate() {
                    let nbx = (hx as i64 + dx) as usize;
                    let nby = (hy as i64 + dy) as usize;
                    let nbz = (hz as i64 + dz) as usize;

                    let visible = is_face_visible(halo, nbx, nby, nbz);

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

/// Заполняет `halo` блоками субчанка и одноблочным кольцом соседей.
///
/// Центр берётся из своего слота, кольцо по X/Z — из соседних чанков через тор
/// (слота нет → воздух), кольцо по Y — из соседних субчанков того же слота
/// (`SUBCHUNKS_X = SUBCHUNKS_Z = 1`, поэтому горизонтальные координаты субчанка
/// совпадают с чанковыми). Ниже `y = 0` соседа нет — ставим непрозрачный сентинел,
/// выше `CHUNK_HEIGHT` — воздух. Семантика совпадает с прежней `is_face_visible`.
#[allow(clippy::too_many_arguments)]
fn fill_halo(
    halo: &mut HaloBuf,
    pool: &ChunkPool,
    //slot: usize,
    chunk_x: i64,
    chunk_z: i64,
    sub_x: usize,
    sub_y: usize,
    sub_z: usize,
    window: &ReadWindowManager,
) {
    // Слоты девяти соседних чанков; индекс `dx + dz * 3`, где dx/dz ∈ {-1, 0, 1}.
    let mut neighbor_slots: [Option<usize>; 9] = [None; 9];
    for dz in 0..3i64 {
        for dx in 0..3i64 {
            let (ncx, ncz) = window.normalize_chunk(chunk_x + dx - 1, chunk_z + dz - 1);
            neighbor_slots[(dx + dz * 3) as usize] = window.slot_for_chunk(ncx, ncz);
        }
    }

    for hz in 0..HALO_SIDE {
        for hx in 0..HALO_SIDE {
            let world_x = (sub_x + hx) as isize - 1;
            let world_z = (sub_z + hz) as isize - 1;
            let dx = world_x.div_euclid(CHUNK_SIDE as isize);
            let dz = world_z.div_euclid(CHUNK_SIDE as isize);
            let local_x = world_x.rem_euclid(CHUNK_SIDE as isize) as usize;
            let local_z = world_z.rem_euclid(CHUNK_SIDE as isize) as usize;

            // `dx`/`dz` ∈ {-1, 0, 1}; +1 приводит их к тем же 0..3, что и в таблице выше.
            let neighbor_slot = neighbor_slots[((dx + 1) + (dz + 1) * 3) as usize];
            let column_base = (local_z * CHUNK_SIDE + local_x) * CHUNK_HEIGHT;

            for hy in 0..HALO_HEIGHT {
                // `hy = 0` → `y = sub_y - 1`; `hy = 17` → `y = sub_y + SUBCHUNK_HEIGHT`.
                let y_signed = sub_y as isize + hy as isize - 1;

                let material_id = if y_signed < 0 {
                    HALO_OPAQUE
                } else if y_signed >= CHUNK_HEIGHT as isize {
                    ids::AIR
                } else {
                    match neighbor_slot {
                        Some(s) => pool.get(s, column_base + y_signed as usize).material_id,
                        None => ids::AIR,
                    }
                };

                halo.set(hx, hy, hz, material_id);
            }
        }
    }
}

fn is_face_visible(halo: &HaloBuf, hx: usize, hy: usize, hz: usize) -> bool {
    is_transparent(halo.get(hx, hy, hz))
}