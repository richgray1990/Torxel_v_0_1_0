//! Буфер запросов теневого окна.

use bevy::prelude::*;
use smallvec::SmallVec;

/// Тип запроса теневого окна
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowRequestKind {
    /// Запросить чанк в теневое окно
    Request,
    /// Освободить чанк из теневого окна
    Release,
}

/// Запрос теневого окна
#[derive(Clone, Copy, Debug)]
pub struct ShadowRequest {
    pub kind: ShadowRequestKind,
    pub chunk_x: usize,
    pub chunk_z: usize,
}

/// Буфер запросов теневого окна
#[derive(Resource, Default)]
pub struct ShadowRequestBuffer {
    pub requests: SmallVec<[ShadowRequest; 32]>,
}

impl ShadowRequestBuffer {
    pub fn push(&mut self, request: ShadowRequest) {
        self.requests.push(request);
    }

    pub fn drain(&mut self) -> impl Iterator<Item = ShadowRequest> + '_ {
        self.requests.drain(..)
    }

    pub fn clear(&mut self) {
        self.requests.clear();
    }
}