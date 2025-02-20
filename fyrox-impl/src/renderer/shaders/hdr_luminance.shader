(
    name: "HdrLuminance",
    resources: [
        (
            name: "frameSampler",
            kind: Texture(kind: Sampler2D, fallback: White),
            binding: 0
        ),
        (
            name: "properties",
            kind: PropertyGroup([
                (name: "worldViewProjection", kind: Matrix4()),
                (name: "invSize", kind: Vector2()),
            ]),
            binding: 0
        ),
    ],
    passes: [
        (
            name: "Primary",

            draw_parameters: DrawParameters(
                cull_face: None,
                color_write: ColorMask(
                    red: true,
                    green: true,
                    blue: true,
                    alpha: true,
                ),
                depth_write: false,
                stencil_test: None,
                depth_test: None,
                blend: None,
                stencil_op: StencilOp(
                    fail: Keep,
                    zfail: Keep,
                    zpass: Keep,
                    write_mask: 0xFFFF_FFFF,
                ),
                scissor_box: None
            ),

            vertex_shader:
                r#"
                    layout (location = 0) in vec3 vertexPosition;
                    layout (location = 1) in vec2 vertexTexCoord;

                    out vec2 texCoord;

                    void main()
                    {
                        texCoord = vertexTexCoord;
                        gl_Position = properties.worldViewProjection * vec4(vertexPosition, 1.0);
                    }
                "#,

            fragment_shader:
                r#"
                    in vec2 texCoord;

                    out float outLum;

                    void main() {
                        float totalLum = 0.0;
                        for (float y = -0.5; y < 0.5; y += 0.5) {
                            for (float x = -0.5; x < 0.5; x += 0.5) {
                                totalLum += S_Luminance(texture(frameSampler, texCoord - vec2(x, y) * properties.invSize).xyz);
                            }
                        }
                        outLum = totalLum / 9.0;
                    }
                "#,
        )
    ]
)