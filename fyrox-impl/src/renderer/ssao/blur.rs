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

use crate::renderer::framework::geometry_buffer::{DrawCallStatistics, ElementRange};
use crate::renderer::framework::GeometryBufferExt;
use crate::{
    core::{math::Rect, scope_profile, sstorage::ImmutableString},
    renderer::{
        framework::{
            error::FrameworkError,
            framebuffer::{Attachment, AttachmentKind, DrawParameters, FrameBuffer},
            geometry_buffer::{GeometryBuffer, GeometryBufferKind},
            gpu_program::{GpuProgram, UniformLocation},
            gpu_texture::{
                Coordinate, GpuTexture, GpuTextureKind, MagnificationFilter, MinificationFilter,
                PixelKind, WrapMode,
            },
            state::PipelineState,
        },
        make_viewport_matrix,
    },
    scene::mesh::surface::SurfaceData,
};
use std::{cell::RefCell, rc::Rc};

struct Shader {
    program: GpuProgram,
    world_view_projection_matrix: UniformLocation,
    input_texture: UniformLocation,
}

impl Shader {
    fn new(state: &PipelineState) -> Result<Self, FrameworkError> {
        let fragment_source = include_str!("../shaders/blur_fs.glsl");
        let vertex_source = include_str!("../shaders/blur_vs.glsl");

        let program = GpuProgram::from_source(state, "BlurShader", vertex_source, fragment_source)?;
        Ok(Self {
            world_view_projection_matrix: program
                .uniform_location(state, &ImmutableString::new("worldViewProjection"))?,
            input_texture: program
                .uniform_location(state, &ImmutableString::new("inputTexture"))?,
            program,
        })
    }
}

pub struct Blur {
    shader: Shader,
    framebuffer: FrameBuffer,
    quad: GeometryBuffer,
    width: usize,
    height: usize,
}

impl Blur {
    pub fn new(state: &PipelineState, width: usize, height: usize) -> Result<Self, FrameworkError> {
        let frame = {
            let kind = GpuTextureKind::Rectangle { width, height };
            let mut texture = GpuTexture::new(
                state,
                kind,
                PixelKind::R32F,
                MinificationFilter::Nearest,
                MagnificationFilter::Nearest,
                1,
                None,
            )?;
            texture
                .bind_mut(state, 0)
                .set_wrap(Coordinate::S, WrapMode::ClampToEdge)
                .set_wrap(Coordinate::T, WrapMode::ClampToEdge);
            texture
        };

        Ok(Self {
            shader: Shader::new(state)?,
            framebuffer: FrameBuffer::new(
                state,
                None,
                vec![Attachment {
                    kind: AttachmentKind::Color,
                    texture: Rc::new(RefCell::new(frame)),
                }],
            )?,
            quad: GeometryBuffer::from_surface_data(
                &SurfaceData::make_unit_xy_quad(),
                GeometryBufferKind::StaticDraw,
                state,
            )?,
            width,
            height,
        })
    }

    pub fn result(&self) -> Rc<RefCell<GpuTexture>> {
        self.framebuffer.color_attachments()[0].texture.clone()
    }

    pub(crate) fn render(
        &mut self,
        state: &PipelineState,
        input: Rc<RefCell<GpuTexture>>,
    ) -> Result<DrawCallStatistics, FrameworkError> {
        scope_profile!();

        let viewport = Rect::new(0, 0, self.width as i32, self.height as i32);

        let shader = &self.shader;
        self.framebuffer.draw(
            &self.quad,
            state,
            viewport,
            &shader.program,
            &DrawParameters {
                cull_face: None,
                color_write: Default::default(),
                depth_write: false,
                stencil_test: None,
                depth_test: false,
                blend: None,
                stencil_op: Default::default(),
            },
            ElementRange::Full,
            |mut program_binding| {
                program_binding
                    .set_matrix4(
                        &shader.world_view_projection_matrix,
                        &(make_viewport_matrix(viewport)),
                    )
                    .set_texture(&shader.input_texture, &input);
            },
        )
    }
}
