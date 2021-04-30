pub(super) mod auth;
pub(super) mod pipeline;
pub(super) mod schema;
pub(super) mod stream;

pub(self) fn pipeline_max_parallel_default() -> u32 {
    50
}
