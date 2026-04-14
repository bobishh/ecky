use csgrs::mesh::Mesh;
use csgrs::sketch::Sketch;

use crate::models::AppError;

pub(super) type IrMesh = Mesh<()>;
pub(super) type IrSketch = Sketch<()>;
pub(super) type LoopPoints = Vec<[f64; 2]>;

pub(super) fn unsupported(details: impl Into<String>) -> AppError {
    AppError::with_details(
        crate::models::AppErrorCode::Validation,
        "Unsupported by Ecky IR v0. Switch the thread engine to FreeCAD and rerender.",
        details.into(),
    )
}

pub(super) fn validation(message: impl Into<String>) -> AppError {
    AppError::validation(message.into())
}
