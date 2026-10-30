//! Модуль рендера воксельного мира.

pub mod camera;
pub mod mesh_builder;
pub mod mesh_storage;
pub mod render_systems;
pub mod entity_pool;

pub use camera::{CameraController, camera_input_system, camera_transform_system};
pub use entity_pool::{
    ChunkEntityPool, PooledChunkEntity, VoxelMaterials, init_voxel_material_and_pool,
};