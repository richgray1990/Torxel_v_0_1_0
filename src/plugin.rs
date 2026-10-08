//! Плагин менеджера чанков.

use bevy::prelude::*;

use crate::render::{
    CameraController, camera_input_system, camera_transform_system,
    init_voxel_material_system,
};
use crate::render::mesh_storage::MeshStorage;
use crate::render::render_systems::{
    process_mesh_events_system, build_meshes_system, swap_meshes_system,
    update_mesh_entities_system, cleanup_mesh_entities_system,
};

use crate::voxel::pool::{ChunkPool, ReadWorld, WriteWorld};
use crate::voxel::topology::TorusTopology;
use crate::io::channels::IoManager;
use crate::io::file_format::WorldHeader;
use crate::io::staging::{ActiveStagingBuffer};
use crate::io::worker::IoWorker;
use crate::manager::{
    ChunkManager, ExclusivePoolRegistry, ReadWindowManager, //SlotMetadata, SlotState,
};
use crate::queues::cell_events::CellEventQueue;
use crate::queues::mesh_queue::MeshUpdateQueue;
use crate::queues::PostSwapDirtyBuffer;
use crate::signals::{ComputePipeline, SwapSignal};
use crate::systems::{
    apply_cell_events_system, chunk_garbage_collector_system, dispatch_dirty_events_system,
    dispatch_loaded_events_system, block_interaction_system, init_highlight_system,
    initial_copy_system, initialize_window_system, poll_background_tasks_system,
    post_swap_copy_system, request_swap_system, swap_pointers_system, update_window_system,
    HighlightState, InitialCopyDone, WindowInitialized,
};
use crate::systems::debug::{debug_report_system, DebugTimer};

/// Системные сеты (уровень 1 — строгая цепочка)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameFlowSet {
    /// Фаза опроса фоновых задач (параллельно: активный + теневой)
    PollPhase,
    /// Начальное копирование
    InitialCopy,
    /// Фаза обновления (параллельно: окно + теневые запросы)
    UpdatePhase,
    /// Обмен указателей активного пула
    SwapPointers,
    /// Передача событий рендеру о загрузке чанков
    DispatchLoadedEvents,
    /// Передача событий рендеру об изменении чанков
    DispatchDirtyEvents,
    /// Окно параллелизма (рендер + обсчёт)
    ParallelWork,
    /// Запрос свапа на следующем кадре
    RequestSwap,
    /// Фаза сборки мусора (параллельно: активный + теневой)
    GCPhase,
}

/// Системные сеты внутри PollPhase (параллельно)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PollPhaseSet {
    PollTasks,
}

/// Системные сеты внутри UpdatePhase (параллельно)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum UpdatePhaseSet {
    UpdateWindow,
}

/// Системные сеты внутри ParallelWork (параллельные ветки)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParallelWorkSet {
    Render,
    PostSwapBranch,
}

/// Системные сеты внутри PostSwapBranch (цепочка)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PostSwapBranchSet {
    PostSwapCopy,
    ComputeBlock,
}

/// Системные сеты внутри ComputeBlock (цепочка)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ComputeBlockSet {
    Subsystems,
    Aggregate,
}

/// Системные сеты внутри GCPhase (параллельно)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GCPhaseSet {
    GarbageCollect,
}

/// Плагин менеджера чанков
pub struct TorxelPlugin {
    pub topology: TorusTopology,
    pub world_path: std::path::PathBuf,
}

impl Plugin for TorxelPlugin {
    fn build(&self, app: &mut App) {
        let header = WorldHeader::new(
            self.topology.chunks_x as u32,
            self.topology.chunks_z as u32,
            crate::voxel::format::CHUNK_HEIGHT as u32,
        );

        let (io_manager, worker_channels) = IoManager::new();
        let _worker = IoWorker::spawn(worker_channels, self.world_path.clone(), header);

        // Регистрация ресурсов
        app.insert_resource(ChunkManager::new(self.topology.clone()));
        app.insert_resource(ReadWindowManager::new(&self.topology));

        app.insert_resource(ReadWorld(ChunkPool::new_max()));
        app.insert_resource(WriteWorld(ChunkPool::new_max()));
        
        
        app.insert_resource(ComputePipeline::default());
        app.insert_resource(io_manager);
        app.insert_resource(header);
        
        app.init_resource::<SwapSignal>();
        app.init_resource::<MeshUpdateQueue>();
        app.init_resource::<CellEventQueue>();
        
        app.init_resource::<PostSwapDirtyBuffer>();
        app.init_resource::<ActiveStagingBuffer>();
        
        app.init_resource::<WindowInitialized>();
        app.init_resource::<InitialCopyDone>();

        app.insert_resource(ExclusivePoolRegistry::new(8, 32));
        
        app.init_resource::<CameraController>();
        app.init_resource::<MeshStorage>();
        app.init_resource::<DebugTimer>();

        app.init_resource::<HighlightState>();

        // ═══════════════════════════════════════════════════════════
        // Уровень 1: строгая цепочка фаз
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (
                GameFlowSet::PollPhase,
                GameFlowSet::InitialCopy,
                GameFlowSet::UpdatePhase,
                
                GameFlowSet::SwapPointers,
                GameFlowSet::DispatchLoadedEvents,
                GameFlowSet::DispatchDirtyEvents,
                GameFlowSet::ParallelWork,
                GameFlowSet::RequestSwap,
                GameFlowSet::GCPhase,
            )
                .chain(),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: PollPhase — параллельно
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (PollPhaseSet::PollTasks)
                .in_set(GameFlowSet::PollPhase),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: UpdatePhase — параллельно
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (UpdatePhaseSet::UpdateWindow)
                .in_set(GameFlowSet::UpdatePhase),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: ParallelWork — параллельные ветки
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (ParallelWorkSet::Render, ParallelWorkSet::PostSwapBranch)
                .in_set(GameFlowSet::ParallelWork),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 3: PostSwapBranch — цепочка
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (PostSwapBranchSet::PostSwapCopy, PostSwapBranchSet::ComputeBlock)
                .chain()
                .in_set(ParallelWorkSet::PostSwapBranch),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 4: ComputeBlock — цепочка
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (ComputeBlockSet::Subsystems, ComputeBlockSet::Aggregate)
                .chain()
                .in_set(PostSwapBranchSet::ComputeBlock),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: GCPhase — параллельно
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (GCPhaseSet::GarbageCollect)
                .in_set(GameFlowSet::GCPhase),
        );

        // ═══════════════════════════════════════════════════════════
        // Регистрация систем
        // ═══════════════════════════════════════════════════════════

        // Startup
        app.add_systems(Startup, (init_voxel_material_system, init_highlight_system, initialize_window_system));

        // PollPhase (параллельно)
        app.add_systems(
            Update,
            poll_background_tasks_system.in_set(PollPhaseSet::PollTasks),
        );
        
        // InitialCopy
        app.add_systems(
            Update,
            initial_copy_system.in_set(GameFlowSet::InitialCopy),
        );

        // UpdatePhase (параллельно)
       // UpdatePhase: ввод камеры -> обновление окна -> трансформ камеры.
        app.add_systems(
            Update,
            (
                camera_input_system,
                update_window_system,
                camera_transform_system,
            )
                .chain()
                .in_set(UpdatePhaseSet::UpdateWindow),
        );
        
        // SwapPointers
        app.add_systems(
            Update,
            swap_pointers_system.in_set(GameFlowSet::SwapPointers),
        );

        // DispatchLoadedEvents
        app.add_systems(
            Update,
            dispatch_loaded_events_system.in_set(GameFlowSet::DispatchLoadedEvents),
        );

        // DispatchDirtyEvents
        app.add_systems(
            Update,
            dispatch_dirty_events_system.in_set(GameFlowSet::DispatchDirtyEvents),
        );

        // ═══════════════════════════════════════════════════════════
        // ParallelWork — две параллельные ветки
        // ═══════════════════════════════════════════════════════════

        // Ветка Render: цепочка систем рендера
        app.add_systems(
            Update,
            (
                process_mesh_events_system,
                build_meshes_system,
                swap_meshes_system,
                update_mesh_entities_system,
                cleanup_mesh_entities_system,
            )
                .chain()
                .in_set(ParallelWorkSet::Render),
        );

        // Ветка PostSwapBranch
        app.add_systems(
            Update,
            post_swap_copy_system.in_set(PostSwapBranchSet::PostSwapCopy),
        );

        app.add_systems(
            Update,
            block_interaction_system.in_set(ComputeBlockSet::Subsystems),
        );

        app.add_systems(
            Update,
            apply_cell_events_system.in_set(ComputeBlockSet::Aggregate),
        );

        // RequestSwap
        app.add_systems(
            Update,
            request_swap_system.in_set(GameFlowSet::RequestSwap),
        );

        // GCPhase (параллельно)
        app.add_systems(
            Update,
            chunk_garbage_collector_system.in_set(GCPhaseSet::GarbageCollect),
        );
        
        app.add_systems(Update, debug_report_system);
    }
}