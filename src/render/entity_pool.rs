use bevy::prelude::*;
use crate::render::VoxelMaterialHandle;

/// Пул сущностей для чанков. Фиксированный размер, переиспользование вместо spawn/despawn.
#[derive(Resource)]
pub struct ChunkEntityPool {
    /// Все сущности пула
    entities: Vec<Entity>,
    /// Какие слоты активны (используются)
    active: Vec<bool>,
    /// Стек свободных индексов для быстрого поиска
    free_stack: Vec<usize>,
}

impl ChunkEntityPool {
    pub const POOL_SIZE: usize = 2048; // Достаточно для окна 16×16 + запас
    
    /// Взять свободную сущность из пула
    pub fn acquire(&mut self) -> Option<usize> {
        self.free_stack.pop()
    }
    
    /// Вернуть сущность в пул
    pub fn release(&mut self, slot: usize) {
        if slot < self.active.len() && self.active[slot] {
            self.active[slot] = false;
            self.free_stack.push(slot);
        }
    }
    
    /// Получить Entity по индексу слота
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

    /// Проверить, активен ли слот
    pub fn is_active(&self, slot: usize) -> bool {
        self.active.get(slot).copied().unwrap_or(false)
    }
    
    /// Активировать слот
    pub fn activate(&mut self, slot: usize) {
        if slot < self.active.len() {
            self.active[slot] = true;
        }
    }
}

pub fn init_voxel_material_and_pool(world: &mut World) {
    // 1. Создаём материал напрямуюную
    let material = StandardMaterial {
        base_color: Color::WHITE,
        ..Default::default()
    };
    
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
    let material_handle = materials.add(material);
    world.insert_resource(VoxelMaterialHandle(material_handle.clone()));
    
    // 2. Создаём пул сущностей
    let pool_size = ChunkEntityPool::POOL_SIZE;
    let mut entities = Vec::with_capacity(pool_size);
    let mut active = Vec::with_capacity(pool_size);
    let mut free_stack = Vec::with_capacity(pool_size);
    
    for i in 0..pool_size {
        let entity = world
            .spawn((
                Transform::default(),
                Visibility::Hidden,
                MeshMaterial3d(material_handle.clone()),
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
    
    info!("✓ Voxel material and chunk entity pool initialized: {} entities", pool_size);
}