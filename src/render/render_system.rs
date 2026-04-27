use crate::render::renderer::Renderer;
use crate::render::transform::Transform;
use crate::ecs::world::World;

/// Draws one mesh per entity that has a Transform component.
/// 
/// Reads the renderer from world resources and the transform from
/// each matching entity. Passes the model matrix to the renderer
/// for each draw call.
pub fn render_system(world: &mut World) {
    let transform_id = world.component_id::<Transform>().unwrap();

    // Transforms are collected into a Vec before the renderer is borrowed.
    // The query holds an immutable borrow on the world, and get_resource_mut
    // requires a mutable borrow. Rust does not allow both at the same time,
    // so the query must be dropped first.
    let matrices: Vec<[[f32; 4]; 4]> = {
        let query = world
            .query_builder()
            .require::<Transform>()
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