//! Пул сущностей и материалы воксельного рендера.

use bevy::prelude::*;

/// Компонент-маркер сущности, взятой из пула.
#[derive(Component)]
pub struct PooledChunkEntity {
    pub slot: usize,
}

/// Три материала для трёх фаз рендера.
#[derive(Resource)]
pub struct VoxelMaterials {
    pub opaque: Handle<StandardMaterial>,
    pub transparent: Handle<StandardMaterial>,
    pub alpha_test: Handle<StandardMaterial>,
}

/// Пул сущностей для чанков.
///
/// Один субчанк теперь может занимать до трёх сущностей:
/// opaque, transparent, alpha_test.
#[derive(Resource)]
pub struct ChunkEntityPool {
    entities: Vec<Entity>,
    active: Vec<bool>,
    free_stack: Vec<usize>,
}

impl ChunkEntityPool {
    /// 2048 субчанков * 3 слоя = 6144 сущности.
    pub const POOL_SIZE: usize = 6144;

    pub fn acquire(&mut self) -> Option<usize> {
        self.free_stack.pop()
    }

    pub fn release(&mut self, slot: usize) {
        if slot < self.active.len() && self.active[slot] {
            self.active[slot] = false;
            self.free_stack.push(slot);
        }
    }

    pub fn entity(&self, slot: usize) -> Option<Entity> {
        self.entities.get(slot).copied()
    }

    pub fn entity_len(&self) -> usize {
        self.entities.len()
    }

    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    pub fn free_len(&self) -> usize {
        self.free_stack.len()
    }

    pub fn is_active(&self, slot: usize) -> bool {
        self.active.get(slot).copied().unwrap_or(false)
    }

    pub fn activate(&mut self, slot: usize) {
        if slot < self.active.len() {
            self.active[slot] = true;
        }
    }
}

pub fn init_voxel_material_and_pool(world: &mut World) {
    // 1. Создаём три материала.
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();

    let opaque = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Opaque,
        ..Default::default()
    });

    let transparent = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        ..Default::default()
    });

    let alpha_test = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Mask(0.5),
        ..Default::default()
    });

    drop(materials);

    world.insert_resource(VoxelMaterials {
        opaque: opaque.clone(),
        transparent: transparent.clone(),
        alpha_test: alpha_test.clone(),
    });

    // 2. Создаём пул сущностей.
    let pool_size = ChunkEntityPool::POOL_SIZE;

    let mut entities = Vec::with_capacity(pool_size);
    let mut active = Vec::with_capacity(pool_size);
    let mut free_stack = Vec::with_capacity(pool_size);

    for i in 0..pool_size {
        let entity = world
            .spawn((
                Transform::default(),
                Visibility::Hidden,
                // Стартовый материал. При acquire будет перезаписан нужным.
                MeshMaterial3d(opaque.clone()),
            ))
            .id();

        entities.push(entity);
        active.push(false);
        free_stack.push(i);
    }

    world.insert_resource(ChunkEntityPool {
        entities,
        active,
        free_stack,
    });

    info!(
        "✓ Voxel materials and chunk entity pool initialized: {} entities",
        pool_size
    );
}