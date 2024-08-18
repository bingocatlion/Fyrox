// Copyright (c) 2019-present Dmitry Stepanov and Fyrox Engine contributors.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use crate::{
    core::{algebra::Vector3, pool::Handle},
    renderer::framework::{
        error::FrameworkError,
        query::{Query, QueryKind, QueryResult},
        state::PipelineState,
    },
    scene::node::Node,
};
use fxhash::FxHashMap;

#[derive(Debug)]
struct PendingQuery {
    query: Query,
    observer_position: Vector3<f32>,
    node: Handle<Node>,
}

#[derive(Debug)]
enum Visibility {
    Undefined,
    Invisible,
    Visible,
}

type NodeVisibilityMap = FxHashMap<Handle<Node>, Visibility>;

/// Volumetric visibility cache based on occlusion query.
pub struct VisibilityCache {
    cells: FxHashMap<Vector3<i32>, NodeVisibilityMap>,
    pending_queries: Vec<PendingQuery>,
    granularity: Vector3<u32>,
    distance_discard_threshold: f32,
}

fn world_to_grid(world_position: Vector3<f32>, granularity: Vector3<u32>) -> Vector3<i32> {
    Vector3::new(
        (world_position.x * (granularity.x as f32)).round() as i32,
        (world_position.y * (granularity.y as f32)).round() as i32,
        (world_position.z * (granularity.z as f32)).round() as i32,
    )
}

fn grid_to_world(grid_position: Vector3<i32>, granularity: Vector3<u32>) -> Vector3<f32> {
    Vector3::new(
        grid_position.x as f32 / (granularity.x as f32),
        grid_position.y as f32 / (granularity.y as f32),
        grid_position.z as f32 / (granularity.z as f32),
    )
}

impl VisibilityCache {
    pub fn new(granularity: Vector3<u32>, distance_discard_threshold: f32) -> Self {
        Self {
            cells: Default::default(),
            pending_queries: Default::default(),
            granularity,
            distance_discard_threshold,
        }
    }

    pub fn world_to_grid(&self, world_position: Vector3<f32>) -> Vector3<i32> {
        world_to_grid(world_position, self.granularity)
    }

    pub fn grid_to_world(&self, grid_position: Vector3<i32>) -> Vector3<f32> {
        grid_to_world(grid_position, self.granularity)
    }

    fn visibility_info(
        &self,
        observer_position: Vector3<f32>,
        node: Handle<Node>,
    ) -> Option<&Visibility> {
        let grid_position = self.world_to_grid(observer_position);

        self.cells
            .get(&grid_position)
            .and_then(|cell| cell.get(&node))
    }

    pub fn needs_occlusion_query(
        &self,
        observer_position: Vector3<f32>,
        node: Handle<Node>,
    ) -> bool {
        let Some(visibility) = self.visibility_info(observer_position, node) else {
            // There's no data about the visibility, so the occlusion query is needed.
            return true;
        };

        match visibility {
            Visibility::Undefined => {
                // There's already an occlusion query on GPU.
                false
            }
            Visibility::Invisible => {
                // The object could be invisible from one angle at the observer position, but visible
                // from another. Since we're using only position of the observer, we cannot be 100%
                // sure, that the object is invisible even if a previous query told us so.
                true
            }
            Visibility::Visible => {
                // Some pixels of the object is visible from the given observer position, so we don't
                // need a new occlusion query.
                false
            }
        }
    }

    pub fn is_visible(&self, observer_position: Vector3<f32>, node: Handle<Node>) -> bool {
        let Some(visibility_info) = self.visibility_info(observer_position, node) else {
            return false;
        };

        match *visibility_info {
            Visibility::Visible
            // Undefined visibility is treated like the object is visible, this is needed because
            // GPU queries are async, and we must still render the object to prevent popping light.
            | Visibility::Undefined => true,
            Visibility::Invisible => false,
        }
    }

    pub fn begin_query(
        &mut self,
        pipeline_state: &PipelineState,
        observer_position: Vector3<f32>,
        node: Handle<Node>,
    ) -> Result<(), FrameworkError> {
        let query = Query::new(pipeline_state)?;
        query.begin(QueryKind::AnySamplesPassed);
        self.pending_queries.push(PendingQuery {
            query,
            observer_position,
            node,
        });

        let grid_position = self.world_to_grid(observer_position);
        self.cells
            .entry(grid_position)
            .or_default()
            .entry(node)
            .or_insert(Visibility::Undefined);

        Ok(())
    }

    pub fn end_query(&mut self) {
        let last_pending_query = self
            .pending_queries
            .last()
            .expect("begin_query/end_query calls mismatch!");
        last_pending_query.query.end();
    }

    pub fn update(&mut self, observer_position: Vector3<f32>) {
        self.pending_queries.retain_mut(|pending_query| {
            if let Some(QueryResult::AnySamplesPassed(query_result)) =
                pending_query.query.try_get_result()
            {
                let grid_position =
                    world_to_grid(pending_query.observer_position, self.granularity);

                let visibility = self
                    .cells
                    .get_mut(&grid_position)
                    .expect("grid cell must exist!")
                    .get_mut(&pending_query.node)
                    .expect("object visibility must be predefined!");

                match visibility {
                    Visibility::Undefined => match query_result {
                        true => {
                            *visibility = Visibility::Visible;
                        }
                        false => {
                            *visibility = Visibility::Invisible;
                        }
                    },
                    Visibility::Invisible => {
                        if query_result {
                            // Override "invisibility" - if any fragment of an object is visible, then
                            // it will remain visible forever. This is ok for non-moving objects only.
                            *visibility = Visibility::Visible;
                        }
                    }
                    Visibility::Visible => {
                        // Ignore the query result and keep the visibility.
                    }
                }

                false
            } else {
                true
            }
        });

        // Remove visibility info from the cache for distant cells.
        self.cells.retain(|grid_position, _| {
            let world_position = grid_to_world(*grid_position, self.granularity);

            world_position.metric_distance(&observer_position) < self.distance_discard_threshold
        });
    }
}
