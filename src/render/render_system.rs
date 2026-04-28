use crate::render::camera::{ActiveCamera, Camera};
use crate::render::renderer::Renderer;
use crate::render::transform::Transform;
use crate::ecs::world::World;

/// Draws one mesh per entity that has a Transform but not a Camera component.
///
/// Reads the active camera resource to build the VP matrix once per frame,
/// then passes a model matrix to the renderer for each matching entity.
///
/// Camera entities are excluded from the mesh draw query because they
/// have no geometry. Once mesh components are added, the query will
/// require those instead.
pub fn render_system(world: &mut World) {
    // Read the current aspect ratio from the renderer.
    let aspect = match world.get_resource::<Renderer>() {
        Some(r) => r.aspect_ratio(),
        None => return,
    };

    // Read the active camera entity.
    let camera_entity = match world.get_resource::<ActiveCamera>() {
        Some(a) => a.entity,
        None => return,
    };

    // Keep the camera aspect ratio in sync with the window size.
    if let Some(camera) = world.get_component_mut::<Camera>(camera_entity) {
        camera.aspect = aspect;
    }

    // Build the VP matrix from the camera projection and its transform.
    //
    // Projection comes from the Camera component; view comes from the
    // Transform on the same entity. VP = projection * view.
    let vp_matrix = {
        let proj = match world.get_component::<Camera>(camera_entity) {
            Some(c) => c.to_projection_matrix(),
            None => return,
        };

        let view = match world.get_component::<Transform>(camera_entity) {
            Some(t) => t.to_view_matrix(),
            None => return,
        };

        mat4_mul(proj, view)
    };

    // Write the VP matrix to the GPU once before any draw calls.
    if let Some(renderer) = world.get_resource_mut::<Renderer>() {
        renderer.set_vp_matrix(vp_matrix);
    }

    // Collect model matrices from all renderable entities.
    //
    // The query borrows the world immutably, so the renderer must be
    // accessed after the query is dropped.
    let transform_id = world.component_id::<Transform>().unwrap();

    let matrices: Vec<[[f32; 4]; 4]> = {
        let query = world
            .query_builder()
            .require::<Transform>()
            .exclude::<Camera>()
            .build();

        query
            .iter()
            .map(|row| {
                row.get::<Transform>(transform_id)
                    .unwrap()
                    .to_model_matrix()
            })
            .collect()
    };

    let renderer = world.get_resource_mut::<Renderer>().unwrap();

    for matrix in matrices {
        renderer.render(matrix);
    }
}

/// Multiplies two column-major 4x4 matrices, returning a * b.
///
/// Applying the result to a vector is equivalent to applying b first,
/// then a. This matches the WGSL convention where `vp * model * position`
/// applies model before vp.
fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0f32; 4]; 4];

    for col in 0..4 {
        for row in 0..4 {
            for k in 0..4 {
                result[col][row] += a[k][row] * b[col][k];
            }
        }
    }

    result
}