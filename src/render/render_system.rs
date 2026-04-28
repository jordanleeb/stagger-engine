use crate::ecs::world::World;
use crate::render::camera::{ActiveCamera, Camera};
use crate::render::debug::DebugDraw;
use crate::render::material::Material;
use crate::render::mesh::Mesh;
use crate::render::renderer::{DrawCall, Renderer};
use crate::render::transform::Transform;

/// Draws one mesh per entity that has a Transform, Mesh, and Material component.
///
/// Reads the active camera resource to build the VP matrix once per frame,
/// collects one DrawCall per renderable entity, takes any pending debug tasks,
/// then submits everything to the renderer in a single render_frame call.
pub fn render_system(world: &mut World) {
    let aspect = match world.get_resource::<Renderer>() {
        Some(r) => r.aspect_ratio(),
        None => return,
    };

    let camera_entity = match world.get_resource::<ActiveCamera>() {
        Some(a) => a.entity,
        None => return,
    };

    if let Some(camera) = world.get_component_mut::<Camera>(camera_entity) {
        camera.aspect = aspect;
    }

    // Build the VP matrix from the camera projection and its transform.
    // VP = projection * view.
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

    // Collect one DrawCall per renderable entity.
    //
    // The query borrows the world immutably, so the renderer must be
    // accessed after the query is dropped.
    let transform_id = world.component_id::<Transform>().unwrap();
    let mesh_component_id = world.component_id::<Mesh>().unwrap();
    let material_id = world.component_id::<Material>().unwrap();

    let draw_calls: Vec<DrawCall> = {
        let query = world
            .query_builder()
            .require::<Transform>()
            .require::<Mesh>()
            .require::<Material>()
            .exclude::<Camera>()
            .build();

        query
            .iter()
            .map(|row| {
                let model_matrix = row
                    .get::<Transform>(transform_id)
                    .unwrap()
                    .to_model_matrix();

                let mesh_id = row.get::<Mesh>(mesh_component_id).unwrap().id;

                let material_color = row.get::<Material>(material_id).unwrap().color;

                DrawCall {
                    model_matrix,
                    material_color,
                    mesh_id,
                }
            })
            .collect()
    };

    // Take all pending debug tasks, leaving the buffer empty for the next frame.
    let debug_tasks = world
        .get_resource_mut::<DebugDraw>()
        .map(|d| d.take())
        .unwrap_or_default();

    let renderer = world.get_resource_mut::<Renderer>().unwrap();
    renderer.render_frame(vp_matrix, &draw_calls, &debug_tasks);
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
